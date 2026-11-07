//! Sums typed into number boxes, and the document's variables they can use.
//!
//! Read from the original:
//! - A box holds a sum, not a number, as soon as it has anything but digits, dots, commas, spaces and tabs,
//!   or a minus sign anywhere but first (`0x14062b9b0`).
//! - Working it out (`0x140628cc0`): every `=` is dropped, then quotes and spaces; `{name}` becomes the
//!   variable's value (`0x14061d8c0`); `P` or `p` becomes 3.1415927, `M` or `m` 25.4 (millimetres in an inch),
//!   `I` or `i` 0.03937 (inches in a millimetre); `W`, `w`, `X` or `x` the job's width, `H`, `h`, `Y` or `y` its
//!   height, `T`, `t`, `Z` or `z` its thickness; each `'` makes all before it feet (`0x140629992`); commas become
//!   dots. What's left is `+ - * / ^ !` and brackets.
//! - Ranks (`0x14062a0af` to `0x14062ade6`): `+ -` 1, `* /` 2, `^ !` 3, brackets 10, numbers 11. A minus first, or
//!   right after `! ( ) * + - / ^`, belongs to the number. The sum is split at its rightmost operator of lowest
//!   rank (`0x14062c2f0`), so every operator goes left to right, and worked out (`0x14062bc10`): `!` multiplies
//!   1 × 2 × … × n.
//! - What can go wrong (`0x14062c540`): an error in the sum, brackets that don't pair up, two dots in one
//!   number, dividing by zero. A name the document doesn't have stops it too (`0x14061d8c0`).
//! - A variable's name starts with a letter, then letters, digits or `_` (`0x14061d2e0`), and is matched
//!   exactly, capitals and all (`0x14061e080`). Each is saved as its name and its value (`0x14061d110`).
//! - The answer shows as `sum = value unit`, to 4 places in inches and 3 in millimetres (`0x140314b90`).

use serde::{Deserialize, Serialize};
use std::cell::RefCell;

pub const PI_TEXT: &str = "3.1415927";
pub const MM_PER_INCH: f64 = 25.4;
pub const INCH_PER_MM: f64 = 0.03937;

/// The original's Document Variables command.
pub const DOCUMENT_VARIABLES_COMMAND: u32 = 33702;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormulaError {
    Expression,
    Brackets,
    Dots,
    DivideByZero,
    Unknown(String),
}

impl FormulaError {
    pub fn text(&self) -> String {
        match self {
            FormulaError::Expression => crate::i18n::tr("This sum can't be worked out.").into(),
            FormulaError::Brackets => crate::i18n::tr("The brackets don't pair up.").into(),
            FormulaError::Dots => crate::i18n::tr("A number has more than one dot.").into(),
            FormulaError::DivideByZero => crate::i18n::tr("It divides by zero.").into(),
            FormulaError::Unknown(name) => tf!("The document has no variable called {}.", name),
        }
    }
}

/// What a sum can refer to: the document's variables and the job's size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub variables: Vec<Variable>,
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    pub metric: bool,
}

thread_local! {
    static CURRENT: RefCell<Context> = RefCell::new(Context::default());
}

/// Set at each frame, so every number box can work out sums with the open document's values.
pub fn set_context(context: Context) {
    CURRENT.with(|c| *c.borrow_mut() = context);
}

pub fn context() -> Context {
    CURRENT.with(|c| c.borrow().clone())
}

/// Whether the text is a sum rather than a plain number (`0x14062b9b0`).
pub fn is_formula(text: &str) -> bool {
    text.chars().enumerate().any(|(i, c)| !(c.is_ascii_digit() || matches!(c, '\t' | ' ' | ',' | '.') || (c == '-' && i == 0)))
}

/// A name a variable may have (`0x14061d2e0`).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(char::is_alphabetic) && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Whether a sum uses this variable.
pub fn uses(text: &str, name: &str) -> bool {
    text.contains(&format!("{{{name}}}"))
}

/// Works the sum out (`0x140628cc0`).
pub fn evaluate(text: &str, context: &Context) -> Result<f64, FormulaError> {
    let mut s: String = text.chars().filter(|c| !matches!(c, '=' | '"' | ' ' | '\t')).collect();
    // The variables first, so their names aren't taken for the letters below.
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or(FormulaError::Expression)?;
        let name = &after[..close];
        if !valid_name(name) {
            return Err(FormulaError::Expression);
        }
        let value = context.variables.iter().find(|v| v.name == name).ok_or_else(|| FormulaError::Unknown(name.to_string()))?.value;
        out.push_str(&format!("({value:.6})"));
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    s = out;
    let size = |v: f64| format!("{v:.6}");
    for (letters, value) in [
        ("Pp", PI_TEXT.to_string()),
        ("Mm", MM_PER_INCH.to_string()),
        ("Ii", INCH_PER_MM.to_string()),
        ("WwXx", size(context.width)),
        ("HhYy", size(context.height)),
        ("TtZz", size(context.thickness)),
    ] {
        for letter in letters.chars() {
            s = s.replace(letter, &value);
        }
    }
    let s = feet(&s).replace(',', ".");
    let mut p = Parser { s: s.as_bytes(), at: 0 };
    let depth = s.bytes().try_fold(0i32, |d, b| {
        let d = d + match b {
            b'(' => 1,
            b')' => -1,
            _ => 0,
        };
        (d >= 0).then_some(d)
    });
    if depth != Some(0) {
        return Err(FormulaError::Brackets);
    }
    let value = p.sum()?;
    if p.at != p.s.len() || !value.is_finite() {
        return Err(FormulaError::Expression);
    }
    Ok(value)
}

/// The feet sign (`0x140629992`): each `'` goes, `)*12` takes its place and `(` goes at the very start, so all
/// that's written before it counts in feet. Burin adds a `+` when a number or a bracket follows straight on, so
/// `5'6` is 66; the original would read `(5)*126`.
pub fn feet(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(i) = s.find('\'') {
        let rest = &s[i + 1..];
        let join = if rest.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '.' | ',' | '(')) { "+" } else { "" };
        s = format!("({}){FEET_TIMES}{join}{rest}", &s[..i]);
    }
    s
}

/// What the feet sign multiplies by: inches in a foot (`0x1421932f8`, the text `)*12`).
pub const FEET_TIMES: &str = "*12";

/// How the answer shows next to the box (`0x140314b90`).
pub fn shown(text: &str, value: f64, unit: &str, metric: bool) -> String {
    if metric { format!("{text} = {value:4.3} {unit}") } else { format!("{text} = {value:4.4} {unit}") }
}

/// `n!` (`0x14062bc10`, kind 9): 1 × 2 × … × n, with n cut to a whole number and 0! = 1. The original asks
/// before working out 1000! or more (`0x142736398`); past 170! the answer is too big to show anyway, so the
/// sum is refused instead.
fn factorial(n: f64) -> Result<f64, FormulaError> {
    if !(0.0..=FACTORIAL_MOST).contains(&n) {
        return Err(FormulaError::Expression);
    }
    Ok((1..=n.trunc() as u32).fold(1.0, |p, k| p * f64::from(k)))
}

/// The largest whole number whose factorial still fits in a number Burin can show.
pub const FACTORIAL_MOST: f64 = 170.0;

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn sum(&mut self) -> Result<f64, FormulaError> {
        let mut v = self.product()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.at += 1;
            let r = self.product()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn product(&mut self) -> Result<f64, FormulaError> {
        let mut v = self.power()?;
        while let Some(op @ (b'*' | b'/')) = self.peek() {
            self.at += 1;
            let r = self.power()?;
            if op == b'/' {
                if r == 0.0 {
                    return Err(FormulaError::DivideByZero);
                }
                v /= r;
            } else {
                v *= r;
            }
        }
        Ok(v)
    }

    /// `^` and `!` share the top rank and, like every operator in the original, are worked out from left to
    /// right: it splits a sum at its rightmost operator of lowest rank (`0x14062c2f0`), so `2^3^2` is 64.
    fn power(&mut self) -> Result<f64, FormulaError> {
        let mut v = self.unary()?;
        loop {
            match self.peek() {
                Some(b'^') => {
                    self.at += 1;
                    let exponent = self.unary()?;
                    v = v.powf(exponent);
                }
                Some(b'!') => {
                    self.at += 1;
                    v = factorial(v)?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn unary(&mut self) -> Result<f64, FormulaError> {
        match self.peek() {
            Some(b'-') => {
                self.at += 1;
                Ok(-self.unary()?)
            }
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64, FormulaError> {
        if self.peek() == Some(b'(') {
            self.at += 1;
            let v = self.sum()?;
            if self.peek() != Some(b')') {
                return Err(FormulaError::Brackets);
            }
            self.at += 1;
            return Ok(v);
        }
        let start = self.at;
        let mut dots = 0;
        while let Some(c) = self.peek().filter(|c| c.is_ascii_digit() || *c == b'.') {
            dots += usize::from(c == b'.');
            self.at += 1;
        }
        if dots > 1 {
            return Err(FormulaError::Dots);
        }
        std::str::from_utf8(&self.s[start..self.at]).ok().and_then(|t| t.parse().ok()).ok_or(FormulaError::Expression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Context {
        Context { variables: vec![Variable { name: "depth".into(), value: 0.75 }, Variable { name: "Tab_2".into(), value: 2.0 }], width: 24.0, height: 18.0, thickness: 0.75, metric: false }
    }

    #[test]
    fn a_plain_number_isnt_a_sum() {
        for text in ["12", "-3.5", "1,5", " 4 "] {
            assert!(!is_formula(text), "{text}");
        }
        for text in ["2*3", "3-1", "W/2", "{depth}", "1e3"] {
            assert!(is_formula(text), "{text}");
        }
    }

    #[test]
    fn sums_use_the_jobs_size_pi_units_and_variables() {
        let c = job();
        assert_eq!(evaluate("W/2", &c), Ok(12.0));
        assert_eq!(evaluate("= h - 1", &c), Ok(17.0));
        assert_eq!(evaluate("{depth}/3", &c), Ok(0.25));
        assert_eq!(evaluate("2*{Tab_2}^2", &c), Ok(8.0));
        assert_eq!(evaluate("-(2+3)*2", &c), Ok(-10.0));
        assert!((evaluate("10*i", &c).unwrap() - 0.3937).abs() < 1e-9);
        assert!((evaluate("p", &c).unwrap() - 3.1415927).abs() < 1e-12);
        assert_eq!(evaluate("1,5*2", &c), Ok(3.0), "a comma is a dot");
    }

    #[test]
    fn powers_and_factorials_go_left_to_right() {
        let c = job();
        assert_eq!(evaluate("2^3^2", &c), Ok(64.0), "the original splits at the rightmost ^");
        assert_eq!(evaluate("3!", &c), Ok(6.0));
        assert_eq!(evaluate("0!", &c), Ok(1.0));
        assert_eq!(evaluate("2*3!", &c), Ok(12.0));
        assert_eq!(evaluate("3!^2", &c), Ok(36.0));
        assert_eq!(evaluate("2^3!", &c), Ok(40320.0), "(2^3)!, same rank");
        assert_eq!(evaluate("4.7!", &c), Ok(24.0), "cut to a whole number");
        assert_eq!(evaluate("2*-3", &c), Ok(-6.0), "a minus after an operator is a sign");
        assert_eq!(evaluate("(-3)!", &c), Err(FormulaError::Expression));
        assert_eq!(evaluate("171!", &c), Err(FormulaError::Expression));
    }

    #[test]
    fn the_feet_sign_counts_whats_before_it_in_feet() {
        let c = job();
        assert_eq!(evaluate("5'", &c), Ok(60.0));
        assert_eq!(evaluate("5'+6", &c), Ok(66.0));
        assert_eq!(evaluate("5'6", &c), Ok(66.0), "Burin adds the + the original leaves out");
        assert_eq!(evaluate("2+3'", &c), Ok(60.0), "everything before it");
        assert_eq!(evaluate("1'2'", &c), Ok(168.0));
        assert!(is_formula("5'"));
    }

    #[test]
    fn what_goes_wrong_is_said() {
        let c = job();
        assert_eq!(evaluate("(2+3", &c), Err(FormulaError::Brackets));
        assert_eq!(evaluate("1.2.3+1", &c), Err(FormulaError::Dots));
        assert_eq!(evaluate("4/(2-2)", &c), Err(FormulaError::DivideByZero));
        assert_eq!(evaluate("{Depth}", &c), Err(FormulaError::Unknown("Depth".into())), "names are matched exactly");
        assert_eq!(evaluate("2*", &c), Err(FormulaError::Expression));
    }

    #[test]
    fn names_start_with_a_letter() {
        assert!(valid_name("depth") && valid_name("Tab_2") && valid_name("é"));
        assert!(!valid_name("2nd") && !valid_name("_x") && !valid_name("a b") && !valid_name(""));
    }

    #[test]
    fn the_answer_shows_to_the_places_of_its_unit() {
        assert_eq!(shown("W/2", 12.0, "in", false), "W/2 = 12.0000 in");
        assert_eq!(shown("W/2", 300.0, "mm", true), "W/2 = 300.000 mm");
    }
}

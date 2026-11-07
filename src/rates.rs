//! Starting points for a tool's speeds, from the material and the tool's size. Burin's own, from the
//! usual chip load rule: feed = chip load per tooth × teeth × spindle speed. General figures for
//! router bits; a tool maker's own are better.

use crate::relief::ToolKind;
use crate::toolpath::Tool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Softwood,
    Hardwood,
    Mdf,
    Plywood,
    Plastic,
    Aluminium,
}

pub const MATERIALS: [Material; 6] = [Material::Softwood, Material::Hardwood, Material::Mdf, Material::Plywood, Material::Plastic, Material::Aluminium];

impl Material {
    pub fn name(self) -> &'static str {
        match self {
            Material::Softwood => crate::i18n::tr("Softwood"),
            Material::Hardwood => crate::i18n::tr("Hardwood"),
            Material::Mdf => crate::i18n::tr("MDF"),
            Material::Plywood => crate::i18n::tr("Plywood"),
            Material::Plastic => crate::i18n::tr("Acrylic and plastics"),
            Material::Aluminium => crate::i18n::tr("Aluminium"),
        }
    }

    /// Chip load in millimetres per tooth for a 3, 6 and 12 mm tool.
    fn chip_loads(self) -> [f64; 3] {
        match self {
            Material::Softwood => [0.08, 0.25, 0.45],
            Material::Hardwood => [0.06, 0.20, 0.40],
            Material::Mdf => [0.10, 0.30, 0.50],
            Material::Plywood => [0.08, 0.25, 0.45],
            Material::Plastic => [0.05, 0.15, 0.25],
            Material::Aluminium => [0.02, 0.05, 0.08],
        }
    }

    /// How deep one pass goes, in tool diameters.
    fn pass_in_diameters(self) -> f64 {
        match self {
            Material::Plastic => 0.5,
            Material::Aluminium => 0.25,
            _ => 1.0,
        }
    }
}

/// Between the three sizes in a straight line, held at the ends.
pub fn chip_load(material: Material, dia_mm: f64) -> f64 {
    let [a, b, c] = material.chip_loads();
    match dia_mm {
        d if d <= 3.0 => a * (d / 3.0).max(0.25),
        d if d <= 6.0 => a + (b - a) * (d - 3.0) / 3.0,
        d if d <= 12.0 => b + (c - b) * (d - 6.0) / 6.0,
        _ => c,
    }
}

/// What a tool's rates would be: same spindle speed (18000 if none), feed from the chip load, plunge
/// a third of that, pass depth from the material. In the tool's own unit.
pub fn suggest(tool: &Tool, material: Material, flutes: u32) -> Tool {
    let mut t = tool.clone();
    let k = if tool.metric { 1.0 } else { 25.4 };
    let dia_mm = tool.dia * k;
    // A V-bit or engraver cuts with a point narrower than its diameter.
    let cutting_mm = if matches!(tool.kind, ToolKind::VBit | ToolKind::Engraving) { (dia_mm * 0.5).max(1.0) } else { dia_mm };
    if t.rpm <= 0.0 {
        t.rpm = 18000.0;
    }
    let feed_mm = chip_load(material, cutting_mm) * flutes.max(1) as f64 * t.rpm;
    t.feed = feed_mm / k;
    t.plunge = t.feed / 3.0;
    t.pass_depth = (dia_mm * material.pass_in_diameters()).max(0.1) / k;
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_follow_the_chip_load_rule() {
        let mut t = crate::toolpath::default_tools().into_iter().find(|t| t.metric).unwrap();
        t.dia = 6.0;
        t.rpm = 18000.0;
        let s = suggest(&t, Material::Mdf, 2);
        assert!((s.feed - 0.30 * 2.0 * 18000.0).abs() < 1e-9, "{}", s.feed);
        assert!((s.plunge - s.feed / 3.0).abs() < 1e-9);
        assert!((s.pass_depth - 6.0).abs() < 1e-9);
        assert!((chip_load(Material::Hardwood, 9.0) - 0.30).abs() < 1e-9);
        let mut inch = t.in_units(false);
        inch.rpm = 0.0;
        let s = suggest(&inch, Material::Aluminium, 3);
        assert_eq!(s.rpm, 18000.0);
        assert!((s.feed * 25.4 - 0.05 * 3.0 * 18000.0).abs() < 1e-6);
    }
}

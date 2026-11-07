//! Help Contents: short pages on each part of the program, in the program itself, so they work
//! offline and in the chosen language. The original opens a separate manual (command 57670).

use crate::app::App;
use crate::i18n::{language, tr, Lang};
use egui::Align2;

/// The original's Help Contents command.
pub const HELP_CONTENTS_COMMAND: u32 = 57670;

/// One page of help, in English and in French.
pub struct Page {
    pub title: &'static str,
    pub text: &'static str,
    pub titre: &'static str,
    pub texte: &'static str,
}

impl Page {
    fn shown(&self) -> (&'static str, &'static str) {
        match language() {
            Lang::French => (self.titre, self.texte),
            Lang::English => (self.title, self.text),
        }
    }
}

pub const PAGES: [Page; 9] = [
    Page {
        title: "Getting Started",
        text: "Start with File > New, or Create a new file on the start screen. Give the size and thickness of the material, \
               where its zero is (a corner or the middle) and whether that zero is on the top of the material or on the table.\n\n\
               You can change all of this later in Edit > Job Setup. Open a file from another program with File > Open: its \
               drawing, layers, toolpaths and notes come in.",
        titre: "Pour commencer",
        texte: "Commencez par Fichier > Nouveau, ou par « Créer un nouveau fichier » sur l'écran d'accueil. Donnez la taille et \
                l'épaisseur de la matière, où se trouve son zéro (un coin ou le milieu), et si ce zéro est sur le dessus de la \
                matière ou sur la table.\n\n\
                Tout cela se change ensuite dans Édition > Configuration du projet. Un fichier d'un autre logiciel s'ouvre avec \
                Fichier > Ouvrir : son dessin, ses couches, ses parcours et ses notes sont repris.",
    },
    Page {
        title: "Drawing",
        text: "The Drawing tab on the left creates and edits shapes: circles, rectangles, polygons, stars, lines, curves and text. \
               Click a shape to select it; drag it to move it, or drag its handles to scale or turn it.\n\n\
               While moving, turning or scaling, type a value and press Enter for an exact result: x,y or a distance for a move, \
               degrees for a turn, a percentage for a scale. Edit Objects joins, trims, offsets and lines up shapes. Layers keep \
               shapes apart and can be hidden.",
        titre: "Dessin",
        texte: "L'onglet Dessin, à gauche, crée et modifie les formes : cercles, rectangles, polygones, étoiles, lignes, courbes et \
                texte. Cliquez sur une forme pour la choisir ; faites-la glisser pour la déplacer, ou tirez ses poignées pour la \
                redimensionner ou la tourner.\n\n\
                Pendant un déplacement, une rotation ou un redimensionnement, tapez une valeur puis Entrée pour un résultat \
                exact : x,y ou une distance pour déplacer, des degrés pour tourner, un pourcentage pour redimensionner. « Modifier \
                les objets » joint, coupe, décale et aligne les formes. Les couches séparent les formes et peuvent être cachées.",
    },
    Page {
        title: "Modelling",
        text: "The Modeling tab builds a 3D shape from closed shapes of your drawing: rounded, angled or flat, added to or \
               taken away from each other. The 3D view shows the result. Toolpaths > 3D Roughing and 3D Finishing carve it.",
        titre: "Modélisation",
        texte: "L'onglet Modélisation construit une forme en 3D à partir des formes fermées du dessin : arrondie, en pente ou \
                plate, ajoutée aux autres ou retirée. La vue 3D montre le résultat. Parcours > Ebauche 3D et Finition 3D la \
                sculptent.",
    },
    Page {
        title: "Toolpaths",
        text: "The Toolpaths tab on the right makes the cuts: profiles around or inside shapes, pockets, V-carving, drilling and 3D. \
               Select the shapes first, then pick a toolpath, a tool and the depths.\n\n\
               Tools live in Toolpaths > Tool Database. Give each tool its flute length, so a cut deeper than the flutes is caught, \
               and use Suggest in the tool to get a feed from the material and the number of flutes.",
        titre: "Parcours",
        texte: "L'onglet Parcours, à droite, crée les coupes : contours autour ou à l'intérieur des formes, poches, gravure en V, \
                perçage et 3D. Choisissez d'abord les formes, puis un parcours, un outil et les profondeurs.\n\n\
                Les outils sont dans Parcours > Base d'outils. Donnez à chaque outil sa longueur de coupe, pour qu'une passe plus \
                profonde que ses dents soit signalée, et utilisez « Proposer » dans l'outil pour obtenir une avance d'après la \
                matière et le nombre de dents.",
    },
    Page {
        title: "Checking Before Cutting",
        text: "Preview Toolpaths carves the material on the screen. Toolpaths > Check Before Cutting lists what could go wrong: \
               a feed of 0, a cut deeper than the flutes, fast moves inside the material, cuts outside the job. A danger stops \
               saving until you choose to save anyway.\n\n\
               In the Toolpaths menu, Toolpath Drawing shows fast moves, plunges and retracts in their own colours.",
        titre: "Vérifier avant de couper",
        texte: "« Aperçu des parcours » usine la matière à l'écran. Parcours > Vérifier avant de couper liste ce qui pourrait mal \
                se passer : une avance à 0, une passe plus profonde que les dents, des déplacements rapides dans la matière, des \
                coupes hors du projet. Un danger bloque l'enregistrement tant que vous ne choisissez pas d'enregistrer quand même.\n\n\
                Dans le menu Parcours, « Dessin des parcours » montre les déplacements rapides, les plongées et les remontées \
                chacun dans sa couleur.",
    },
    Page {
        title: "Saving for the Machine",
        text: "Save Toolpaths writes the file the machine reads. Choose the post processor that matches your controller. \
               Create Job Sheet gives a page with the tools, the order and the time.\n\n\
               If a cut stopped half way, Toolpaths > Start Again From a Cut writes a file that begins at the cut you choose. \
               Check the first moves before you run it.",
        titre: "Enregistrer pour la machine",
        texte: "« Enregistrer les parcours » écrit le fichier que lit la machine. Choisissez le post-processeur qui correspond à \
                votre commande. « Créer une fiche de travail » donne une page avec les outils, l'ordre et la durée.\n\n\
                Si une coupe s'est arrêtée en route, Parcours > Reprendre à partir d'un passage écrit un fichier qui commence au \
                passage choisi. Vérifiez les premiers mouvements avant de le lancer.",
    },
    Page {
        title: "Files and Backups",
        text: "File > Project Backups lists the earlier versions kept each time you save, and brings one back. \
               File > Import Toolpaths from Other Programs reads a machine file made elsewhere; it stays as it is. \
               File > Print makes a PDF of the drawing.",
        titre: "Fichiers et sauvegardes",
        texte: "Fichier > Sauvegardes du projet liste les versions précédentes gardées à chaque enregistrement, et en remet une. \
                Fichier > Importer des parcours d'autres programmes lit un fichier machine fait ailleurs ; il reste tel quel. \
                Fichier > Imprimer fait un PDF du dessin.",
    },
    Page {
        title: "Keyboard",
        text: "Help > Keyboard Shortcuts lists every shortcut and lets you change them. Escape stops what you are doing; \
               Delete removes the selected shapes.",
        titre: "Clavier",
        texte: "Aide > Raccourcis clavier liste tous les raccourcis et permet de les changer. Échap arrête ce que vous faites ; \
                Suppr efface les formes choisies.",
    },
    Page {
        title: "Getting Help",
        text: "Help > Send Feedback writes a message about a problem or an idea, with the details that help. \
               Help > Details for Support gives a text to paste when someone asks what computer and version you have.",
        titre: "Obtenir de l'aide",
        texte: "Aide > Envoyer un retour prépare un message sur un problème ou une idée, avec les détails utiles. \
                Aide > Détails pour l'assistance donne un texte à coller quand on vous demande votre ordinateur et votre version.",
    },
];

impl App {
    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr("Help Contents"))
            .id(egui::Id::unique("help-contents"))
            .open(&mut open)
            .collapsible(false)
            .default_size([660.0, 380.0])
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        for (i, page) in PAGES.iter().enumerate() {
                            if ui.selectable_label(self.help_page == i, page.shown().0).clicked() {
                                self.help_page = i;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let (title, text) = PAGES[self.help_page.min(PAGES.len() - 1)].shown();
                        ui.heading(title);
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            ui.label(text);
                        });
                    });
                });
            });
        self.help_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::PAGES;

    #[test]
    fn every_help_page_is_written_in_both_languages() {
        for page in PAGES {
            for text in [page.title, page.text, page.titre, page.texte] {
                assert!(!text.trim().is_empty(), "{}", page.title);
                assert!(!text.contains("  "), "a line break left spaces in {}", page.title);
            }
            assert_ne!(page.text, page.texte, "{}", page.title);
        }
    }
}

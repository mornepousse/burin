<div align="center">

<img src="release/burin.png" width="150" alt="Le logo de Burin : un marteau frappe une fraise plantée dans un bloc de bois">

# Burin

**Dessinez, puis usinez.**

Un logiciel de bureau pour dessiner des pièces, préparer des parcours d'outil
et écrire les fichiers de commande d'une machine CNC.

Linux · Windows · Français · English

</div>

![Une pièce usinée dans l'aperçu 3D de Burin, dans une planche de hêtre](docs/apercu-3d.png)

> [!WARNING]
> **Burin n'a encore jamais piloté de vraie machine.** Une erreur dans un fichier de coupe peut casser
> une fraise ou abîmer une pièce. Vérifiez chaque fichier dans un simulateur, puis faites-le tourner à
> vide, fraise bien au-dessus de la matière, avant de couper.

## Ce qu'il sait faire

| | |
|---|---|
| **Dessiner** | Lignes, courbes, arcs, cercles, ellipses, rectangles, polygones, étoiles, spirales, texte, cotes, contraintes d'esquisse |
| **Modifier** | Déplacer, tourner et redimensionner à la souris (avec une valeur exacte tapée au clavier), décaler, souder, couper, aligner, copier en série, lisser les tracés |
| **Organiser** | Calques, groupes et **pièces** : on sélectionne un seul trou sans rien dégrouper, mais la pièce se déplace d'un bloc |
| **Modeler** | Composants 3D, reliefs importés, cliparts |
| **Usiner** | Contour, poche, perçage, gravure, gravure en V, ébauche et finition 3D, prisme, texture, moulure, cannelure, chanfrein |
| **Préparer** | Rampes, entrées et sorties, attaches, ordre de coupe, fusion de parcours, modèles de parcours réutilisables |
| **Vérifier** | Aperçu 3D de la coupe (rapides, plongées et remontées de couleurs différentes), estimation du temps, **vérification avant coupe** (vitesses à 0, outil trop court, rapides dans la matière, attaches trop fines…), fiche de travail |
| **Reprendre** | Après un arrêt, un fichier qui repart d'un passage choisi d'un parcours |
| **Écrire** | G-code générique, GRBL, Mach3, PlanetCNC, et vos post-processeurs `.pp` |
| **Échanger** | Lit les projets `.crv3d` et les fichiers G-code d'autres logiciels, importe SVG, DXF, STL et images, exporte SVG, EPS, PDF, STL, imprime via un PDF à taille réelle |

Le détail de ce qui manque est à la fin de cette page.

## En images

<table>
<tr>
<td width="50%"><img src="docs/poignees.png" alt="Une forme sélectionnée, avec ses poignées de taille et de rotation"><br><b>Poignées</b> : un clic sur la sélection les affiche. <kbd>Alt</kbd> tourne par pas de 15°, <kbd>Maj</kbd> agrandit depuis le centre.</td>
<td width="50%"><img src="docs/pieces.png" alt="Une pièce et sa liste dans l'onglet Couches"><br><b>Pièces</b> : nommées, listées, et enregistrables seules dans leur propre fichier machine.</td>
</tr>
<tr>
<td><img src="docs/preferences.png" alt="La fenêtre des préférences, avec la langue et les raccourcis"><br><b>Préférences</b> : anglais ou français, et toutes les touches modifiables.</td>
<td><img src="docs/fiche-de-travail.png" alt="Une fiche de travail avec le dessin et le réglage de la matière"><br><b>Fiche de travail</b> : le dessin, le réglage de la matière et chaque parcours, sur une seule page.</td>
</tr>
</table>

## Installer

### Linux

```sh
chmod +x Burin-*-x86_64.AppImage
./Burin-*-x86_64.AppImage
```

Ensuite, `Aide > Ajouter au menu des applications` place Burin dans le menu de votre bureau et lui
fait ouvrir les fichiers `.burin`. Un projet peut aussi être donné en argument, ou glissé sur la fenêtre.

### Windows

Lancez `Burin-*-windows-x86_64.exe`, sans installation. Il n'est pas signé : Windows peut demander une
confirmation (`Informations complémentaires > Exécuter quand même`). Il n'a été essayé que sous Wine et
sur un PC Windows 11.

### Depuis le code

Il faut [Rust](https://rustup.rs).

```sh
cargo run --release
cargo test
```

`release/build.sh` fabrique l'AppImage et le `.exe` dans `dist/`. Il ne demande que Nix.

## Si quelque chose ne va pas

Votre travail non enregistré est copié toutes les deux minutes et vous est proposé au lancement suivant
si Burin s'est mal fermé.

| Dans le menu Aide | |
|---|---|
| **Envoyer un retour** | Un problème, une idée ou une question |
| **Rapports de plantage** | Ce que Burin a noté quand il s'est arrêté |
| **Détails pour l'assistance** | La version, le système, la carte graphique et le dernier démarrage, à copier |

**Burin n'envoie rien tout seul.** Les rapports restent sur votre ordinateur. Si vous choisissez d'en
signaler un, un brouillon s'ouvre dans votre navigateur : vous voyez tout ce qu'il contient, et il ne part
que si vous le validez. Votre travail, ses noms de fichiers et le nom de votre dossier personnel n'y
figurent jamais.

<img src="docs/retour.png" alt="La fenêtre pour envoyer un retour" width="640">

## Ce qui manque encore

- **Essais réels :** rien n'a été vérifié sur une machine.
- **Menus :** une trentaine d'entrées restent grisées, surtout des outils de modélisation 3D et des vues
  (multi-faces, enroulée, ombres).
- **Calculs :** les parcours ne sont pas toujours identiques à ceux enregistrés dans les projets `.crv3d`, notamment l'ordre de coupe et les
  poches, qui lèvent l'outil plus souvent.

Les règles de calcul, avec ce qui a été vérifié et ce qui reste estimé, sont dans
[`MECHANICS.md`](MECHANICS.md).

## Le dépôt

| | |
|---|---|
| `src/` | Le programme, en Rust avec [egui](https://github.com/emilk/egui) |
| `release/` | Le logo et ce qui fabrique les versions à distribuer |
| `docs/` | Les images de cette page |
| `MECHANICS.md`, `mechanics.json` | Les règles de calcul et leurs valeurs |

Aucun fichier de travail réel n'est inclus : `.gitignore` écarte les projets et les fichiers machine.


## Licence

Aucune pour l'instant : tous droits réservés. La liste des bibliothèques utilisées et de leurs licences
accompagne chaque version, dans `THIRD-PARTY.txt`.

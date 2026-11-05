# Burin

Logiciel de bureau pour dessiner des pièces, préparer des parcours d'outil et écrire les fichiers
de commande d'une machine CNC. Il lit aussi les projets `.crv3d` et les post-processeurs (`.pp`).

Projet privé en cours de développement : rien n'a encore été validé sur une vraie machine. Vérifiez
chaque fichier dans un simulateur ou au-dessus de la matière avant de couper.

## Lancer

Il faut Rust (https://rustup.rs).

```sh
cargo run --release
cargo test
```

## Contenu

- `src/` : le programme.
- `MECHANICS.md` et `mechanics.json` : les règles de calcul reprises du logiciel d'origine, avec ce qui
  a été vérifié et ce qui reste estimé.

Aucun fichier de travail réel n'est inclus : `.gitignore` écarte les projets et les fichiers machine.

## Licence

Aucune pour l'instant : tous droits réservés.

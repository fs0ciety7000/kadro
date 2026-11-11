# AGENTS.md

Règles communes à tous les agents de code (Claude Code, Codex, Cursor...). Format neutre : rien ici n'est propre à Claude.

## Contexte

- **Nom** : Kropa (commande `kropa`)
- **Type** : application de bureau Rust, Linux (GNOME/KDE Wayland d'abord) et macOS 15+, licence GPL-3.0-or-later
- **Résumé en une phrase** : capture d'écran, annotation et envoi vers la destination choisie par l'utilisateur
  (Zipline, S3, SFTP, API HTTP), avec copie du lien, local d'abord et tout scriptable.

## Commandes

```bash
install: rustup show   # installe la toolchain de rust-toolchain.toml
dev:     cargo run -p kropa -- --help
test:    cargo nextest run --workspace --locked && cargo test --doc --workspace --locked
lint:    cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo deny --locked check
build:   cargo build --workspace --locked
```

## Structure du dépôt

```
.
├── AGENTS.md, CLAUDE.md
├── DESIGN.md                # direction visuelle (lot D2)
├── Cargo.toml, Cargo.lock   # workspace
├── crates/
│   ├── kropa-core           # config, chemins, modèles de nom, erreurs
│   ├── kropa-doc            # document d'annotation, format .kropa
│   ├── kropa-render         # rendu (tiny-skia, cosmic-text)
│   ├── kropa-capture        # backends de capture et de presse-papiers Linux et macOS
│   ├── kropa-upload         # destinations, client HTTP unique, S3, SFTP
│   ├── kropa-store          # SQLite (historique, file), secrets
│   ├── kropa-keychain       # Keychain macOS (un élément), courtier `kropa __store-broker`, protocole (lot 1.8m)
│   ├── kropa-i18n           # Fluent FR/EN
│   ├── kropa-cli            # paquet `kropa` : la CLI
│   ├── kropa-gtk            # interface Linux (binaire kropa-gtk)
│   └── kropa-macos          # interface macOS (binaire kropa-app)
├── docs/                    # brief, décisions, plan, propositions d'équipe, cli.md, security/, brand/, design/
├── .github/workflows/ci.yml # fmt, clippy, tests Linux + macOS 15, MSRV, cargo-deny, zizmor
└── deny.toml, renovate.json
```

## Règles de travail

1. Lis avant d'écrire : ouvre les fichiers concernés avant de les modifier. `docs/decisions.md` prévaut sur les
   propositions d'équipe.
2. Plus petit changement qui résout le problème. Pas de refactor non demandé.
3. Suis le style existant (nommage, formatage, structure).
4. Ajoute ou mets à jour les tests avec chaque changement de comportement.
5. Lance `test` et `lint` avant de conclure ; rapporte les résultats réels.
6. Ne touche jamais à : fichiers d'environnement locaux, `target/`, fichiers de lock modifiés à la main.
7. Tâche de plus de ~5 fichiers ou changement d'architecture : propose d'abord un plan.
8. Dépendances : dernière version stable vérifiée sur crates.io (jamais de mémoire), licence compatible
   GPL-3.0-or-later (`cargo deny` l'impose), actions par SHA ; Renovate tient le reste à jour.
9. Jamais de succès simulé : une fonction impossible ou dégradée sur une plateforme renvoie un statut explicite et son
   repli documenté (`docs/platform-*.md`).
10. Local d'abord : aucune requête réseau non configurée, aucune télémétrie. Un seul client HTTP, construit par
    `kropa-upload`. Horloge injectable dans le code testé.
11. Toute chaîne visible passe par Fluent (`kropa-i18n`), en français et en anglais.
12. `unsafe` interdit hors du code macOS (`kropa-macos`, backend macOS de `kropa-capture`, modules `system`, `spawn` et
    `apple` de `kropa-keychain`), et toujours justifié (`// SAFETY:`).

## Sécurité

- Aucun secret dans le code, les logs ou les commits. Secrets des destinations : trousseau système, repli fichier `age`
  (voir `docs/security/secrets.md`) ; jamais en argument de commande ni en variable d'environnement.
- Valide toutes les entrées externes (configs, `.sxcu`, réponses HTTP) ; requêtes SQL paramétrées uniquement.
- Confirmation avant toute action destructive (suppression, fusion sur `main`, publication d'une release).

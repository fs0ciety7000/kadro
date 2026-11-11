# Stack

> Validée par l'utilisateur le 11/11/2026 (V1 et V2). Détail et justification : `docs/architecture.md`, `docs/decisions.md`.

- **Types** : application de bureau Rust (registre : `backend-rust` le plus proche, pas de type « desktop » dans le kit),
  plus `brand` et `design-ui` pour l'identité et l'interface.
- **Stack** : workspace Cargo ; cœur commun (config TOML, SQLite via rusqlite, Fluent, tiny-skia + cosmic-text,
  reqwest, object_store, russh-sftp, keyring + repli age) ; Linux : gtk4-rs + libadwaita, ashpd (portails), PipeWire
  plus tard ; macOS 15+ : objc2 (AppKit), ScreenCaptureKit. CLI `kropa` + processus résident, IPC socket Unix JSON-RPC.
- **Licence** : GPL-3.0-or-later.
- **CI** : GitHub Actions, `ubuntu-24.04` + `macos-15` (dépôt public : minutes gratuites). Mises à jour : Renovate.
- **Distribution** : `.deb`, `.rpm`, `.dmg` (certificat auto-signé), tap Homebrew personnel, archives. Ni Flathub ni AUR.
- **Profils de skills** : `rust` (via `kit project add backend-rust`).
- **Agents** : voir `CLAUDE.md`, rubrique « Équipe ».

## Équipement

| Source | Nom | Pourquoi |
|---|---|---|
| kit (`backend-rust`) | skill `rust-backend` | conventions Rust du kit |
| apollographql/skills | skill `rust-best-practices` (MIT) | idiomes, clippy, erreurs ; relu, aucune consigne risquée |
| affaan-m/ecc | skill `rust-testing` | tests unitaires, intégration, propriétés, couverture ; relu |
| écrit pour le projet | agents `linux-desktop-dev`, `macos-desktop-dev` | replis par bureau Linux ; objc2, TCC, signature macOS |
| kit | agents `architect`, `backend-dev`, `qa-tester`, `code-reviewer`, `security-reviewer`, `devops`, `brand-identity`, `ui-designer`, `design-critic`, `copywriter` | copiés dans `.claude/agents/` avec une section « Ce projet » |

Écartés : skills GTK4 de skills.sh (moins de 150 installations ; `z-rust-gtk4` repose sur Relm4, écarté par
l'Architecture).

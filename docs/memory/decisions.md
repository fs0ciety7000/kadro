# Décisions

> Une entrée par décision durable, la plus récente en haut. Format : date, décision, raison, alternative écartée.

- **2026-11-11** — Mises à jour de dépendances par **Renovate** (gabarit du kit, épingle les actions par SHA) plutôt
  que Dependabot (proposé dans `quality-release.md`). Raison : convention du kit, un seul outil.
- **2026-11-11** — Décisions de conception validées : voir `docs/decisions.md` (macOS 15, `age`, Imgur en modèle,
  vidéo retirée, identité A, nom Kropa, dépôt public, .deb/.rpm/Homebrew sans Flathub, GPL-3.0-or-later) et les dix
  décisions de `docs/plan.md` (object_store, IPC socket JSON-RPC, SQLite unique…).
- **2026-11-11** — Binaires de l'interface : `kropa-gtk` sous Linux, `kropa-app` sous macOS (dans le `.app`). Raison :
  Cargo ne conditionne pas un binaire par OS ; deux binaires `kropa-app` dans le workspace entrent en collision.
- **2026-11-11** — Langue : précédence `--lang` > `language` de la config > `LC_ALL`/`LC_MESSAGES`/`LANG` > `en`.
  Raison : un réglage explicite de l'utilisateur prime sur l'environnement (revue de code 1.1-1.3, point 7).

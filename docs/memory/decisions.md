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
- **2026-11-11** — Capture macOS via `objc2-screen-capture-kit` 0.3.2 (et crates `objc2-*`), pas la crate
  `screencapturekit` préférée par `architecture.md` : son `build.rs` compile un pont Swift, exclu sans décision
  validée. `unsafe` confiné à `kropa-capture/src/macos/`.
- **2026-11-11** — Fournisseur crypto de rustls : **aws-lc-rs** (recommandation retenue sur « Continue » de
  l'utilisateur, réversible) à la place de Graviola. Raison : mûr, audité, FIPS, repli selon le processeur (Graviola
  exclut les CPU sans ADX/BMI2/AVX2 et le Raspberry Pi 4). Conséquence : la vérification de compilation macOS depuis
  Linux peut ne plus passer (C compilé) ; la CI `macos-15` compile nativement.
- **2026-11-11** — Magasin de secrets : un élément par secret sous Linux (Secret Service, crate `secret-service`, pas
  `keyring` qui force `unlock_all`) ; **un seul élément** Keychain sous macOS (document v2 complet) pour tenir la
  décision U1 « une seule invite par mise à jour » (ACL liée au `cdhash`, une invite par élément sinon).
- **2026-11-11** — Exceptions `deny.toml` RUSTSEC-2026-0370/-0371/-0372 (« unmaintained », zbus 5 via
  `secret-service` 5.2.0) : à retirer à la sortie de `secret-service` 6.0.0 stable, revue au plus tard le 2027-02-28.

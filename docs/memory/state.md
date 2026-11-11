# État

> Réécrit à la fin de chaque session de travail : où on en est, ce qui bloque, prochaines étapes (3 max).

- **Fait** : conception validée ; lot 0 ; lot 1 tâches 1.1-1.5, 1.7-1.11 et leurs suites de revue (1.3b, 1.7b, 1.8b,
  1.8r, 1.8m, 1.9b) : CLI, capture Linux/macOS, envoi HTTP + Zipline, secrets (Secret Service, Keychain un seul
  élément via `kropa __store-broker`), liens échappés, journal expurgé, presse-papiers (data-control, NSPasteboard,
  GNOME honnête), compose Zipline + test « zéro requête ». 473 tests verts, CI verte Linux + macOS 15 (sauf
  GitGuardian : faux positifs).
- **En cours** : D2.4 (relecture des chaînes FR/EN, glossaire). PR brouillon fs0ciety7000/kropa#1 vers `main`.
- **Bloquant** : 3 incidents GitGuardian à marquer faux positifs par l'utilisateur (39943012, 39943013, 39960789).
- **Prochaines étapes** : 1) finir D2.4 ; 2) essais réels 1.12 par l'utilisateur (GNOME, KDE, Mac, check-lists
  `docs/testing/manual-*.md`) ; 3) lot 2 (sélection, profils, S3, coffre) après validation.

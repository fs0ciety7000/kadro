# État

> Réécrit à la fin de chaque session de travail : où on en est, ce qui bloque, prochaines étapes (3 max).

- **Fait** : lot 1 complet côté code (essais réels KDE + Zipline OK ; Mac à faire). Lot 2 : profils `kropa dest
  add|edit|remove|set-default|export` (avec suites de revue : autorisation calculée contre le magasin), IPC
  `kropa-ipc` + `fdguard`, modèle de sélection, moteur S3 (`object_store` sur le client unique, testé sur Garage),
  magasin du coffre `age` (`vault.rs`). 685 tests verts, CI verte Linux + macOS (sauf GitGuardian : faux positifs).
- **En cours** : revue de sécurité du coffre 2.8a (lancée avant le passage en rigueur standard). PR brouillon
  fs0ciety7000/kropa#1.
- **Bloquant** : rien côté code. Utilisateur : 3 incidents GitGuardian à marquer, essais Mac (1.6, 1.12), réponse sur
  le format de lien par destination.
- **Prochaines étapes** : 1) 2.C `dest add --type s3` et envoi CLI ; 2) 2.D commandes du coffre (`use`, `migrate`,
  `--password-fd`, ajouter les raisons `vault_*` à `ALL_REASONS` de `kropa-upload`) ; 3) 2.B sélection de zone
  (serveur `kropa-gtk`, overlay GTK, `capture area`).

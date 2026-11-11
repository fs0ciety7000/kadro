# État

> Réécrit à la fin de chaque session de travail : où on en est, ce qui bloque, prochaines étapes (3 max).

- **Fait** : conception validée (équipes, décisions, identité A, maquettes, modèle de secrets, spec CLI) ; lot 0 ;
  lot 1 tâches 1.1-1.5, 1.7 et leurs corrections de revue (1.3b, 1.7b). 283 tests verts, CI verte Linux + macOS 15.
- **En cours** : rien (PR brouillon fs0ciety7000/kropa#1 ouverte vers `main`).
- **Bloquant** : choix du fournisseur TLS (aws-lc-rs recommandé) ; faux positifs GitGuardian à marquer par
  l'utilisateur.
- **Prochaines étapes** : 1) fournisseur TLS selon la décision ; 2) tâche 1.8 (stockage des secrets, courtier
  Keychain) ; 3) tâches 1.9-1.11 puis essais réels 1.12.

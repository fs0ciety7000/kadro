# Relais

> Réécrit à la fin de chaque session (`/handoff`), relu au début de la suivante (`/resume`, hook de démarrage).
> Court et concret : la prochaine session doit pouvoir repartir sans rien relire d'autre.

- **Date** : 2026-11-11
- **Fait cette session** :
  - Conception complète, décisions validées, renommage **Kropa**, identité, maquettes, `DESIGN.md`, spec CLI,
    modèle de secrets ; lot 0 (workspace, CI Linux + macOS 15, Renovate, équipe d'agents).
  - Lot 1, toutes les tâches de code (1.1-1.5, 1.7-1.11) et D2.4, avec leurs revues de sécurité et de code
    (`docs/reviews/`) : CLI, capture portail/ScreenCaptureKit, envoi HTTP + Zipline + `.sxcu`, TLS aws-lc-rs, secrets
    (Secret Service ; Keychain en un seul élément via `kropa __store-broker`), liens échappés, journal expurgé,
    presse-papiers (data-control avec relecture, NSPasteboard, GNOME `unavailable`), compose Zipline derrière TLS
    local, test « zéro requête » (`unshare -rn` + `strace`), textes FR/EN relus et glossaire.
  - 473 tests verts ; CI verte Linux + macOS 15 (tous les tests macOS exécutés), sauf GitGuardian (faux positifs).
- **En cours** : rien. Branche `claude/adoring-pascal-u36glt`, PR brouillon
  [fs0ciety7000/kropa#1](https://github.com/fs0ciety7000/kropa/pull/1) vers `main`, surveillée (contrôle programmé).
- **Prochaine action exacte** : essais réels **1.12** par l'utilisateur (check-lists `docs/testing/manual-linux.md`,
  `manual-macos.md`, `integration.md`), puis cadrage du **lot 2** (sélection, profils, S3, coffre `age`).
- **Bloquant / questions ouvertes pour l'utilisateur** :
  - Marquer faux positifs les incidents GitGuardian 39943012, 39943013, 39960789.
  - Tâches à lui : 0.8 réserver le crate `kropa`, 0.9 recherche de marques, 1.6 certificat auto-signé + test TCC,
    1.12 essais réels. Fusionner la PR Engram fs0ciety7000/engram#22.
  - À trancher : code de sortie quand `--copy` explicite échoue (aujourd'hui 0 + avertissement) ; X11 seul sans
    presse-papiers ; journal `-vv` en anglais seulement ; espaces insécables FR dans la CLI ; mot « lot » visible dans
    des messages ; `unsafe` hors macOS (règle 12) pour un mineur de la revue 1.8.
- **À savoir** :
  - Hook claude-kit : faux positifs sur certains mots (fichiers d'environnement, `*.key`, `env`) ; passer par
    Write/Edit, ne jamais contourner pour un vrai secret.
  - Hook « commit and push » : les commits `wip` intermédiaires peuvent rougir la CI ; seul le commit de fin fait foi.
  - Après redémarrage du conteneur, `git remote get-url origin` peut revenir à `kadro` : remettre
    `https://github.com/fs0ciety7000/kropa`.
  - Disque serré (~3 Go libres) : supprimer `target/debug/incremental` avant les gros builds.
  - Valeurs de test ressemblant à des secrets : construites à l'exécution ; dans les compose, passer par `secrets:`.
  - macOS : sockets AF_UNIX (`shutdown`, `setsockopt` après fermeture) se comportent autrement que sous Linux.

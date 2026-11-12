# Relais

> Réécrit à la fin de chaque session (`/handoff`), relu au début de la suivante (`/resume`, hook de démarrage).
> Court et concret : la prochaine session doit pouvoir repartir sans rien relire d'autre.

- **Date** : 2026-11-12
- **Fait cette session** :
  - Lot 1 terminé côté code et essayé en vrai sur KDE Plasma 6.8 (capture, refus, presse-papiers, envoi Zipline réel,
    `kropa doctor --reset-screenshot-permission`).
  - Lot 2 : plan `docs/lots/lot-2.md` (décisions D1-D13 validées « recommandé partout ») ; IPC `kropa-ipc` (socket
    0700, UID du pair, activation D-Bus, `fdguard` CLOEXEC, tests à deux vrais UID) ; modèle de sélection ; `kropa dest
    …` (bloquant B1 corrigé : ce qui « suit » est calculé contre le magasin ; jamais de déplacement d'adresse sans
    question, même magasin injoignable) ; moteur S3 (`object_store` sans feature `aws`, client unique, Garage derrière
    Caddy, test réel) ; magasin du coffre `age` (`crates/kropa-store/src/vault.rs`).
  - Plan allégé (12/11, demande utilisateur via la session claude-kit) : une tâche par fonctionnalité livrable (2.A-2.E,
    3.A-3.B, 4.A-4.F, 5.A-5.E), revues et vérifications séparées retirées.
- **En cours** : revue de sécurité de 2.8a (coffre) lancée avant le changement de règle ; si son rapport n'est pas dans
  `docs/reviews/2026-11-12-security-2.8a.md`, ne pas la relancer (rigueur standard) — vérifier soi-même.
  PR brouillon [fs0ciety7000/kropa#1](https://github.com/fs0ciety7000/kropa/pull/1), CI verte sauf GitGuardian.
- **Prochaine action exacte** : 2.C — `dest add --type s3 --bucket … --region … [--endpoint] [--path-style]
  [--prefix] [--public-base-url | --presign] [--access-key-id] [--secret-stdin|--secret-fd] [--default]`, puis envoi
  Garage de bout en bout par la CLI (`tests/integration/run.sh test --profile s3`). Ensuite 2.D, puis 2.B.
- **Règles du kit (12/11/2026, claude-kit#28)** : rigueur standard (pas de relecteur séparé sauf demande ; vérifier soi-même
  diff, tests, lint, build) ; une PR par fonctionnalité ; Engram seulement à la demande ; pas de `docker build` pour
  vérifier un changement.
- **Bloquant / questions ouvertes pour l'utilisateur** :
  - Format de lien par destination (`link_format`, utilisé quand `--copy` est absent) : l'ajouter (recommandé) ou le
    retirer de la spec.
  - Marquer faux positifs les incidents GitGuardian 39943012, 39943013, 39960789.
  - Essais Mac (1.6 certificat auto-signé, 1.12) ; GNOME non vérifié (pas d'accès).
  - Exceptions `deny.toml` RUSTSEC-2026-0387 (crypto-bigint via `age`) et -0374 (basic-toml) acceptées par
    l'utilisateur, revue au plus tard le 2027-02-28.
- **À savoir** :
  - Disque serré (~5 Go) : supprimer `target/debug/incremental` et les vieux binaires de `target/debug/deps` avant les
    gros builds. Dossiers de scratchpad laissés par des relecteurs (suppression refusée par les permissions).
  - Après redémarrage du conteneur : `git remote set-url origin https://github.com/fs0ciety7000/kropa` ; `dockerd`
    peut être arrêté.
  - Hook « commit and push » : commits `wip` fréquents, CI rouge sur eux ; seul le dernier commit fait foi.
  - Les cas `trycmd` sont isolés du trousseau réel (`DBUS_SESSION_BUS_ADDRESS` inexistant, `XDG_RUNTIME_DIR` privé).
  - macOS : sockets AF_UNIX (`shutdown`, `setsockopt` après fermeture), `sed -i`/`stat` BSD dans les scripts de test.

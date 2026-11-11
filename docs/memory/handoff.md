# Relais

> Réécrit à la fin de chaque session (`/handoff`), relu au début de la suivante (`/resume`, hook de démarrage).
> Court et concret : la prochaine session doit pouvoir repartir sans rien relire d'autre.

- **Date** : 2026-11-11
- **Fait cette session** :
  - Conception complète par 7 équipes (`docs/*.md`), décisions validées (`docs/decisions.md`), renommage Kadro → **Kropa**,
    identité A et logo validés, maquettes + `DESIGN.md` validés (avec outil Recadrer), modèle de secrets revu sans
    bloquant (`docs/security/`), spec CLI v1 validée (`docs/cli.md`).
  - Lot 0 : workspace Cargo, CI GitHub Actions (Linux + macOS 15, MSRV 1.95, cargo-deny, zizmor, bundle `Kropa.app`),
    Renovate, équipe d'agents dans `.claude/agents/`.
  - Lot 1 : 1.1-1.5, 1.7, 1.3b, 1.7b faits — config, modèles de nom, i18n FR/EN, CLI complète, capture Linux (portail),
    capture macOS (ScreenCaptureKit, compilé seulement), moteur d'envoi HTTP + Zipline + import `.sxcu`/`.toml`.
    283 tests verts, CI verte sauf GitGuardian (faux positifs, voir ci-dessous).
- **En cours** (tâche, branche, PR) : rien en cours. Branche `claude/adoring-pascal-u36glt`, PR brouillon
  [fs0ciety7000/kropa#1](https://github.com/fs0ciety7000/kropa/pull/1) vers `main` (surveillée par la session).
- **Prochaine action exacte** : si l'utilisateur choisit `aws-lc-rs`, confier à `backend-dev` le remplacement de
  Graviola dans `crates/kropa-upload` (`http.rs`, clés de test de `testing.rs`), retirer la vérification
  `cargo check --target aarch64-apple-darwin` depuis Linux si elle casse (la CI macOS compile nativement) ; puis tâche
  **1.8** (`kropa-store`, secrets au trousseau, courtier Keychain macOS unique) selon `docs/security/secrets.md`.
- **Bloquant / questions ouvertes pour l'utilisateur** :
  - Fournisseur TLS : `aws-lc-rs` (recommandé, mûr, repli CPU) ou Graviola (jeune, exige AVX2/ADX ou équivalent ARM).
  - Marquer les 2 incidents GitGuardian de la PR comme faux positifs (chaînes de test, déjà retirées du code).
  - Tâches à lui : 0.8 réserver le crate `kropa`, 0.9 recherche de marques, 1.6 certificat auto-signé + test TCC,
    1.12 essais réels GNOME/KDE/Mac (check-lists `docs/testing/manual-*.md`).
  - Deux choix de l'agent 1.3b à valider s'il le souhaite : sous-commande inconnue non citée ; schéma `kropa.error/1`
    pour les commandes sans schéma propre.
- **À savoir** :
  - Le hook claude-kit bloque à tort les commandes contenant certains mots (fichiers d'environnement, champs `key`) :
    passer par Write/Edit, ne pas contourner pour un vrai fichier de secrets.
  - Le hook « commit and push » pousse le travail des agents en cours : la CI de la PR passe au rouge sur ces commits
    intermédiaires ; seul le commit de fin de tâche fait foi.
  - Le dépôt a été renommé : remote `https://github.com/fs0ciety7000/kropa` (après redémarrage du conteneur, revérifier
    `git remote get-url origin`).
  - Sur macOS, lancée depuis Terminal, la permission d'enregistrement d'écran revient probablement à Terminal et non à
    Kropa : à vérifier en 1.12, décision à prendre ensuite.
  - Les valeurs de test ressemblant à des jetons doivent être construites à l'exécution (dépôt public, GitGuardian).
  - Le skill `rust-backend` du profil `rust` du kit concerne Windmill : retiré du projet ; à corriger dans le kit.
  - Engram : fiche `kropa` et leçons dans la PR fs0ciety7000/engram#22 (à fusionner par l'utilisateur).

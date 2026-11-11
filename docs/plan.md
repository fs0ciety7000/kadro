# Plan de livraison — Kropa

> État : **validé par l'utilisateur le 11/11/2026 (V2).** Rédigé par `architect` à partir de `decisions.md` (prioritaire) et
> des documents d'équipe. Vidéo et GIF retirés. Développeur seul, sans délai.
> Chaque PR : `code-reviewer` puis `qa-tester` ; `security-reviewer` en plus si elle touche aux secrets, au réseau, aux
> dépendances, à la signature ou à la publication. Les tâches **[Utilisateur]** ne se délèguent pas.

## Équipe

| Agent | Rôle ici | Modèle / effort | Source |
|---|---|---|---|
| `architect` | conception des lots, `CLAUDE.md`, spec CLI, modèle de secrets | opus / high | kit |
| `backend-dev` | cœur Rust : config, CLI, rendu, document, envoi, store, IPC | sonnet / medium | kit |
| `linux-desktop-dev` | GTK4/libadwaita, portails (ashpd), Wayland, Secret Service, `.desktop` | sonnet / high | à écrire pour le projet |
| `macos-desktop-dev` | objc2/AppKit, ScreenCaptureKit, TCC, Keychain, `.app`, codesign | sonnet / high | à écrire pour le projet |
| `qa-tester` | tests, instantanés d'image, check-lists manuelles | sonnet / medium | kit |
| `code-reviewer` | relecture de chaque PR | sonnet / high | kit |
| `security-reviewer` | secrets, réseau, dépendances, signature, release | opus / xhigh | kit |

Appui ponctuel (kit) : `brand-identity` (D1), `ui-designer` puis `design-critic` (D2, 3.9, 5.7), `copywriter` (FR/EN,
docs), `devops` (CI, paquets, release).

## Décisions proposées (recommandation en premier)

1. **S3 : `object_store` 0.14.2** plutôt que `aws-sigv4` + reqwest : multipart, présignature, XML et reprises déjà
   maintenus. Garde-fous : identifiants statiques seulement (jamais `from_env` ni IMDS), trait `S3Backend`.
2. **Presse-papiers GNOME en v0.1** : lien sur stdout et message « non copié » ; copie fiable en v0.2 (image, focus de
   l'overlay) et v0.4 (lien, notification « Lien prêt, Copier »). Mutter n'a pas data-control.
3. **Paquet CLI nommé `kropa`** (`crates/kropa-cli`) : réservation du crate et `cargo install kropa` coïncident.
4. **`kropa-vision` créée au lot 7 seulement** (pas de crate morte).
5. **IPC : socket Unix JSON-RPC 2.0** plutôt que D-Bus de session.
6. **Une seule base SQLite** (`kropa-store`) pour l'historique et la file d'envoi.
7. **Certificat auto-signé créé dès la v0.1** pour tester tôt la survie des permissions TCC.
8. **Linux minimal : Debian 13, Ubuntu 24.04 LTS, Fedora stable** (fixe GTK4/libadwaita minimales ; à vérifier).
9. **Binaire macOS universel**, partie Intel testée seulement sur `macos-15-intel` (limite documentée).
10. **App ID `io.github.<compte>.Kropa`** (requis par les portails même sans Flathub).

## Lot 0 — Fondations
Dépend de : rien. En parallèle : D1, S.
- [ ] 0.1 **[Utilisateur]** Renommer le dépôt GitHub `kadro` → `kropa`. Fin : remote à jour, ancienne URL redirige.
- [ ] 0.2 Créer `linux-desktop-dev` et `macos-desktop-dev` dans `.claude/agents/`. Agent : session principale. Fin : relus et validés.
- [x] 0.3 Workspace Cargo : `kropa-core`, `kropa-doc`, `kropa-render`, `kropa-capture`, `kropa-upload`, `kropa-store`, `kropa-i18n`, `kropa` (CLI), `kropa-gtk`, `kropa-macos` (binaire `kropa-app`, sous `cfg`). Édition 2024, `rust-toolchain.toml`, MSRV, `[workspace.dependencies]`, `Cargo.lock`, `LICENSE` GPL-3.0-or-later, SPDX. Agent : `backend-dev`. Fin : `cargo build --workspace --locked` et `cargo test` verts sur Linux et macOS ; `kropa --version` répond.
- [x] 0.4 (local vert ; durée et passage macOS à mesurer à la première PR) CI GitHub Actions : fmt, clippy `-D warnings`, nextest + doctests sur `ubuntu-24.04` et `macos-15`, MSRV, `cargo-deny` (licences), `zizmor` ; actions épinglées par SHA, `permissions: contents: read`, Renovate (`renovate.json`, à la place de Dependabot). Agent : `devops`. Fin : PR verte en < 12 min ; une dépendance AGPL fait échouer `deny`.
- [x] 0.5 `CLAUDE.md` et `AGENTS.md` (stack, commandes, règles : local d'abord, client HTTP unique, horloge injectable, jamais de succès simulé, secrets ni en argument ni en variable d'environnement, Fluent dès la première chaîne, `unsafe` confiné à macOS), `docs/memory/`. Agent : `architect`. Fin : relus par l'utilisateur.
- [ ] 0.6 Spécification CLI v1 `docs/cli.md` (`capture screen|output|area`, `upload`, `dest`, `queue`, `secrets`, `history`, `edit`, `doctor`, `version` ; `--upload[=profil]`, `--no-upload`, `--copy url|markdown|html|none`, `--json`, `--output`, `--name`, `--delay`, `--last` ; codes de sortie, schéma JSON). Agents : `architect`, `copywriter`. Fin : validée par l'utilisateur.
- [x] 0.7 Kadro → Kropa dans `docs/` (historique conservé), `.kadro` → `.kropa` ; mettre à jour `uploads.md` (age, plus de vidéo). Agent : `copywriter`. Fin : `grep -ri kadro docs/` ne renvoie que l'historique.
- [ ] 0.8 **[Utilisateur]** Réserver le crate `kropa` (squelette 0.0.x). Dépend de 0.3.
- [ ] 0.9 **[Utilisateur]** Recherche de marques (classes 9 et 42) et de domaines ; résultat dans `decisions.md`.

## Lot D1 — Mot-symbole « kropa » (identité A)
Dépend de : rien. En parallèle : 0, S, 1.
- [x] D1.1 Dessiner « kropa » sur la grille A (`p` à créer), `logo*.svg`, taille minimale, `preview.html`. Agent : `brand-identity`. Fin : validé par l'utilisateur.
- [x] D1.2 `brand.md` réduit à A (B archivée), exemples de ton avec `kropa`. Agent : `brand-identity`.
- [ ] D1.3 Critique logo et icône à 16/32/80 px (dock GNOME, Launchpad ; risque « icône de terminal »). Agent : `design-critic`. Fin : aucun bloquant.

## Lot S — Modèle de secrets (avant tout code de stockage)
Dépend de : rien. En parallèle : 0, D1.
- [x] S.1 `docs/security/secrets.md` : trousseau (Secret Service, Keychain et ACL liées à la signature) ; repli `age` (scrypt, `zeroize`, `0600`, écriture atomique) ; bascule jamais silencieuse ; origines autorisées stockées avec le secret ; `{secret:}`, redirections, HTTPS, import `.sxcu`, journaux expurgés, `--password-fd`, mode non interactif ; modèle de menaces. Agent : `architect`.
- [ ] S.2 Revue. Agent : `security-reviewer`. Fin : points classés bloquants ou non.
- [ ] S.3 Corrections, puis **[Utilisateur]** validation.

## Lot D2 — Maquettes et `DESIGN.md` (avant toute interface)
Dépend de : D1 pour le logo. En parallèle : lot 1. Bloque : 2.3, 2.4, 3.5, 3.6, 4.10, 4.11.
- [ ] D2.1 Maquettes libadwaita et AppKit, clair/sombre, FR/EN : overlay (loupe, dimensions, clavier, multi-écran), éditeur (outils à une touche), notifications (« Lien copié », « Lien prêt, Copier », « Envoi échoué »), réglages (destinations, test, import `.sxcu`, raccourcis), coffre `age`, file d'attente, premier lancement et permissions, miniature (macOS, KDE) et variante notification (GNOME) ; puis `DESIGN.md`. Agent : `ui-designer`. Fin : fichiers dans `docs/design/`.
- [ ] D2.2 Icônes d'outils (grille 20 px, trait 1,75 px). Agent : `ui-designer`.
- [ ] D2.3 Critique et corrections. Agent : `design-critic`. Fin : aucun bloquant ; **[Utilisateur]** valide.
- [ ] D2.4 Chaînes FR et EN en `.ftl`. Agent : `copywriter`.

## Lot 1 — v0.1 Cœur scriptable (capture → fichier → envoi → lien)
Dépend de : 0 ; S pour 1.8. En parallèle : D2 ; 1.4 et 1.5 ensemble.
- [ ] 1.1 `kropa-core` : config TOML, chemins, modèles de nom (`proptest` : aucune sortie hors du dossier), erreurs et codes de sortie, horloge injectable. Agent : `backend-dev`.
- [ ] 1.2 `kropa-i18n` : Fluent branché sur la CLI. Agent : `backend-dev`. Fin : `LANG=fr` / `LANG=en` distincts.
- [ ] 1.3 CLI clap conforme à `docs/cli.md`. Agent : `backend-dev`. Fin : fixtures `trycmd` vertes.
- [ ] 1.4 Capture Linux : trait de capacités, portail Screenshot (`screen`, `output`), `kropa doctor`, `.desktop` + Registry (`scripts/dev-install.sh`). Agent : `linux-desktop-dev`. Fin : test sur portail factice en CI ; refus du portail = code d'erreur dédié.
- [ ] 1.5 Capture macOS : `SCScreenshotManager`, TCC, `.app` minimal. Agent : `macos-desktop-dev`. Fin : `.app` construit en CI ; capture réussie sur le Mac.
- [ ] 1.6 **[Utilisateur]** Certificat auto-signé (20 ans, sauvegarde `age` hors ligne) ; vérifier que la permission survit à deux builds. Fin : résultat dans `decisions.md`.
- [ ] 1.7 `kropa-upload` : client HTTP unique, destinations TOML (gabarits, JSONPath, redirections même origine, HTTPS obligatoire avec secret, réponse ≤ 1 Mio), préréglage Zipline, import `.sxcu`. Agent : `backend-dev`. Fin : tests `wiremock` + `insta` verts.
- [ ] 1.8 `kropa-store` + secrets au trousseau (origines liées), `kropa secrets status`. Dépend de S.3. Agent : `backend-dev`. Fin : tests gnome-keyring (CI) et trousseau temporaire macOS ; `security-reviewer` sans bloquant.
- [ ] 1.9 Liens URL/Markdown/HTML échappés ; journaux expurgés. Agent : `backend-dev`. Fin : un test prouve qu'aucun secret n'apparaît dans les journaux.
- [ ] 1.10 Presse-papiers : `NSPasteboard`, data-control (KDE, wlroots), message honnête sur GNOME. Agents : plateformes. Fin : statut = effet réel.
- [ ] 1.11 Compose de test (Zipline + PostgreSQL, Garage, OpenSSH ; digests épinglés), job d'intégration, test « zéro requête » (`unshare -rn`). Agents : `devops`, `qa-tester`.
- [ ] 1.12 **[Utilisateur]** Essai réel GNOME, KDE, Mac.
**Critère** : sur les trois plateformes, fichier enregistré et lien Zipline réel sur stdout ; copié sur KDE et macOS ; refus explicite sur GNOME ; « zéro requête » vert.

## Lot 2 — v0.2 Sélection, profils, S3, coffre
Dépend de : 1 et D2.
- [ ] 2.1 IPC socket JSON-RPC (`hello` versionné, 0600) ; la CLI lance `kropa-app` à la demande. Agent : `backend-dev`.
- [ ] 2.2 Modèle de sélection commun (souris, flèches, Maj+flèches, `R` dernière zone, Échap, délai) et loupe. Agent : `backend-dev`. Fin : tests sans écran.
- [ ] 2.3 Overlay GTK (image figée par écran, layer-shell KDE/wlroots, plein écran GNOME, échelles mixtes, copie d'image sous focus). Agent : `linux-desktop-dev`. Fin : check-list GNOME et KDE à 100 % et 200 %.
- [ ] 2.4 Overlay AppKit (sans bord, au-dessus, un par écran, Retina, exclus de la capture). Agent : `macos-desktop-dev`.
- [ ] 2.5 `kropa capture area [--last] [--delay]`. Agent : `backend-dev`.
- [ ] 2.6 Profils et défaut, `kropa dest …`, jeton via stdin/TTY (refusé en argument), modèle Imgur dans `docs/examples/`. Agent : `backend-dev`.
- [ ] 2.7 S3 via `object_store` (endpoint, path/virtual-host, préfixe, `public_base_url`, présignature ≤ 7 j en option). Agent : `backend-dev`. Fin : tests Garage ; `security-reviewer` sans bloquant.
- [ ] 2.8 Repli `age` : `kropa secrets unlock`, `--password-fd`. Dépend de S.3. Agent : `backend-dev`. Fin : `security-reviewer` sans bloquant.
- [ ] 2.9 Job hebdomadaire Sway headless. Agents : `devops`, `qa-tester`.
- [ ] 2.10 **[Utilisateur]** Essai du parcours sans annotation sur GNOME, KDE, Mac.

## Lot 3 — v0.3 Éditeur
Dépend de : 2 et D2.2.
- [ ] 3.1 `kropa-doc` : objets, styles, annuler/rétablir ; format `.kropa` v1 (ZIP ; jamais envoyé) ; `docs/format-kropa.md` ; fixtures figées. Agent : `backend-dev`.
- [ ] 3.2 `kropa-render` : formes, crayon, texte, flou fort, pixellisation, masque, compteurs, surligneur ; export PNG sans métadonnées. Agent : `backend-dev`.
- [ ] 3.3 Instantanés d'image (Δ 3/255, 0,1 %) Linux et macOS. Agents : `qa-tester`, `devops`.
- [ ] 3.4 Machine d'états de l'éditeur (raccourcis à une touche, Tab, Suppr, Ctrl+Z, Ctrl+U). Agent : `backend-dev`.
- [ ] 3.5 Coquille éditeur GTK (AT-SPI). Agent : `linux-desktop-dev`.
- [ ] 3.6 Coquille éditeur AppKit (NSAccessibility). Agent : `macos-desktop-dev`.
- [ ] 3.7 `kropa edit <fichier>`. Agent : `backend-dev`.
- [ ] 3.8 Performance sur une capture 5K (< 16 ms par image en glisser). Agent : `qa-tester`.
- [ ] 3.9 Critique de l'éditeur réel. Agent : `design-critic`.
- [ ] 3.10 **[Utilisateur]** Parcours complet sans souris sur les trois plateformes.

## Lot 4 — v0.4 Usage quotidien
Dépend de : 3.
- [ ] 4.1 Résident Linux (`GtkApplication` unique, démarrage auto, icône ksni sur KDE ; GNOME sans icône sans extension). Agent : `linux-desktop-dev`.
- [ ] 4.2 Résident macOS (`LSUIElement`, `NSStatusItem`). Agent : `macos-desktop-dev`.
- [ ] 4.3 Raccourcis globaux (portail GlobalShortcuts ; `global-hotkey` sur macOS, refus d'Option seule) ; repli « raccourci du bureau → `kropa capture area` » documenté. Agents : plateformes.
- [ ] 4.4 Notifications avec actions ; « Lien prêt, Copier » sur GNOME. Agents : plateformes. Fin : jamais de faux « copié ».
- [ ] 4.5 File SQLite (états, reprise au redémarrage, backoff, sans sonde réseau), `kropa queue …`. Agent : `backend-dev`.
- [ ] 4.6 Historique minimal `kropa history list [--search] --json`. Agent : `backend-dev`.
- [ ] 4.7 Session du coffre dans le résident (15 min, verrouillage avec l'écran). Agents : `backend-dev`, plateformes. Fin : `security-reviewer` sans bloquant.
- [ ] 4.8 Retrait des métadonnées sur la copie envoyée. Agent : `backend-dev`. Fin : fixtures EXIF GPS vérifiées.
- [ ] 4.9 SFTP (russh, `known_hosts` strict). Agent : `backend-dev`. Fin : tests OpenSSH ; `security-reviewer` sans bloquant.
- [ ] 4.10 Réglages graphiques simples. Agents : plateformes.
- [ ] 4.11 Premier lancement et permissions expliquées. Agents : plateformes.
- [ ] 4.12 **[Utilisateur]** Deux semaines d'usage réel.

## Lot 5 — v1.0 Paquets et docs
Dépend de : 4.
- [ ] 5.1 `.deb` et `.rpm` (conteneurs Debian 13, Fedora), `.desktop`, MetaInfo, icônes. Agent : `devops`.
- [ ] 5.2 `.dmg` universel (`lipo`, `codesign` auto-signé, `hdiutil`). Agents : `devops`, `macos-desktop-dev`.
- [ ] 5.3 Workflow de release (environnement `release` approuvé à la main, `SHA256SUMS`, provenance, `cargo vendor`, `git-cliff`, `cargo-about`). Agent : `devops`. Fin : `security-reviewer` sans bloquant.
- [ ] 5.4 Tap Homebrew (formule compilée, `.app`). Agent : `devops` ; **[Utilisateur]** crée `homebrew-tap` et teste `brew install`.
- [ ] 5.5 Should si prêts : miniature flottante (macOS, KDE, wlroots ; notification sur GNOME), réouverture `.kropa`, wlroots et X11 au mieux. Agents : plateformes. Fin : livré ou reporté dans `compat.md`.
- [ ] 5.6 Docs FR/EN : README, installation, « Signature macOS et ses conséquences », `compat.md`, référence CLI, `CHANGELOG.md`. Agent : `copywriter`.
- [ ] 5.7 Audit final et critique finale. Agents : `security-reviewer`, `design-critic`.
- [ ] 5.8 **[Utilisateur]** Check-list manuelle, fusion, étiquette, release v1.0.

## Après v1
- **Lot 6, v1.1 Rendu soigné** : fenêtre propre, habillage et préréglages, projecteur, historique graphique.
- **Lot 7, v1.2 Lecture d'image** : `kropa-vision`, OCR (Vision, Tesseract), QR, détection assistée des secrets, pipette, règle.
- **Plus tard** : capture défilante, épinglage (pas sur GNOME), chaîne d'actions libre, envoi par morceaux, suppression distante, crates.io, COPR/OBS.

## Incohérences relevées entre documents (non tranchées)
- Flathub, AppImage, PKGBUILD encore cités par `quality-release.md`, `platform-linux.md`, `product.md` : ce plan suit `decisions.md`.
- Tap Homebrew : v1.x selon `quality-release.md`, v1 selon `decisions.md` : ce plan suit `decisions.md`.
- Critère v0.1 de Produit (lien copié sur GNOME) irréalisable : voir décision 2.
- macOS impose un `.app` même pour la CLI (TCC) : ajouté en 1.5.
- Noms de commandes divergents entre documents : unifiés par 0.6.
- `uploads.md` dépassé (Argon2id, vidéo) : corrigé en 0.7.
- macOS 15 validé, mais sa justification (vidéo) a disparu : rien à changer.
- Résident GNOME sans icône de barre système, alors que Produit en promettait une.
- Envoi par morceaux : reporté après v1 (sans vidéo, les fichiers restent petits).

## Points d'attention
1. Le lot 1 n'est crédible que si 1.6 (certificat et TCC) tient sur le Mac ; sinon, revoir la distribution macOS avant v0.2.
2. Portail Screenshot appelé depuis une CLI sans App ID : non vérifié, à tester dès 1.4 sur un vrai GNOME.
3. `object_store` est en 0.x : trait `S3Backend` et test « zéro requête » indispensables.
4. Deux coquilles natives = risque n° 1 ; la tâche macOS d'un lot peut glisser d'un lot, avec validation de l'utilisateur.
5. Seules les versions d'`object_store` et `aws-sigv4` sont vérifiées ; toutes les autres le seront à leur ajout.

# Kropa : qualité et distribution

> Proposition d'équipe du 11/11/2026. En cas de contradiction, docs/decisions.md prévaut.

> Équipe Qualité et distribution. Proposition à valider, aucun code ni fichier de CI écrit. « Kadro » était le nom de
> travail (historique) ; le nom retenu est Kropa. État vérifié en ligne le **11 novembre 2026** (sources et versions en fin de document, `[n]`).
> Respecte `decisions.md` : pas de vidéo ni de GIF, macOS ≥ 15, repli `age`, Imgur en modèle, pas de signature Apple,
> pas d'AUR, développeur seul.

## 0. Synthèse

| Décision | Choix |
|---|---|
| Lanceur de tests | `cargo-nextest` 0.9.151 ; instantanés texte `insta` 1.49 ; CLI `assert_cmd` 2.2 + `trycmd` 1.2 |
| Rendu | comparaison d'images avec tolérance, polices de test livrées dans le dépôt, sur Linux x86_64 **et** macOS arm64 |
| Envoi | vrais serveurs en conteneurs (Zipline 4.8.3, Garage v2.5.0, OpenSSH) + `wiremock` 0.6.5 pour le HTTP générique |
| Sans écran (Linux) | portail réel + **backend factice** à chaque PR ; Sway headless chaque semaine ; GNOME/KDE réels : à la main |
| CI | GitHub Actions, `ubuntu-24.04` + `macos-15` à chaque PR, < 12 min ; `cargo-deny` 0.20.2 seul pour avis et licences |
| v1.0 | GitHub Releases (`.dmg` universel signé par certificat auto-signé, `.deb`, `.rpm`, archives CLI) + **Flathub** |
| Après v1.0 | tap Homebrew (formule compilée), crates.io, dépôts COPR/OBS ; AppImage seulement sur demande |
| Publication | SemVer, Conventional Commits, `git-cliff` 2.14.2 pour le brouillon, changelog relu à la main |
| Multimédia | ni GStreamer ni FFmpeg ; seul `pipewire` 0.10.1 (libpipewire) restera, pour ScreenCast, **après** v1 |

## 1. Stratégie de tests

Principe : tout ce qui n'est pas une coquille graphique ou un appel système de capture se teste sans écran (le cœur ne
dépend d'aucune bibliothèque graphique, cf. architecture §6). Les tests doivent prouver, jamais simuler, qu'une
fonction marche ; un test qui ne peut pas tourner est **marqué ignoré** avec la raison, pas réussi en silence.

### 1.1 Lanceur

`cargo test` (rien à installer, sortie peu lisible, aucune reprise) contre **`cargo-nextest` 0.9.151** [1]
(parallélisme par test, profils, JUnit ; doctests à lancer à part). **Choix : nextest**, profil `ci` sans reprise (un
test instable est un bogue), profil `it` (intégration) avec 1 reprise et délai de 60 s.

### 1.2 Cœur (unitaires et propriétés)

| Zone | Ce qu'on vérifie | Moyen |
|---|---|---|
| Modèles de nom (`{date}-{host}`) | rendu, dates fixées (horloge injectée), caractères interdits, **aucune sortie hors du dossier** (`..`, `/`, octets nuls) | tables de cas + `proptest` 1.12 (chaîne arbitraire → chemin toujours sous la racine) |
| Gabarits de destination (`{json:…}`, `{regex:…}`, `{secret:…}`) | analyse, erreurs localisées, secrets refusés dans les gabarits de réponse et le nom (uploads §4.5) | tables + `proptest` (l'analyseur ne panique jamais) |
| Conversion `.sxcu` → TOML | corpus de `.sxcu` réels (Zipline, Chevereto, XBackBone, etc.) aux jetons remplacés ; secrets extraits et jamais recopiés ; refus explicites (URLShortener…) | instantanés `insta` du TOML produit et du rapport d'import |
| File d'envoi | machine d'états (uploads §5 : `queued` … `cancelled`), redémarrage (`running` → `queued`), échéances de reprise | SQLite dans un `tempfile`, horloge factice, transitions énumérées |
| Nettoyage des métadonnées | PNG/JPEG témoins avec EXIF GPS, `tEXt`, `iTXt`, `tIME` : retirés ; `sRGB`/`iCCP` gardés | fixtures binaires + vérification octet par octet |
| Format `.kropa` | lecture de chaque version de schéma archivée (`tests/fixtures/kropa/v1…`) | fixtures, jamais régénérées |
| Secrets (repli `age`) | chiffrement/déchiffrement, mauvaise phrase, fichier tronqué | unitaires ; le vrai trousseau en §1.6 |
| « Local d'abord » | **aucune requête réseau sans destination configurée** (critère v0.1 de Produit) | test Linux qui lance la CLI dans un espace de noms réseau vide (`unshare -rn`) et vérifie succès + journal sans tentative ; côté code, un seul constructeur de client HTTP, compteur en test |

Fuzzing (`cargo-fuzz` sur gabarits et `.sxcu`) : après v1.0.

### 1.3 Rendu des annotations par instantanés d'image

| Option | Pour | Contre |
|---|---|---|
| A. `insta` en binaire | déjà utilisé, revue intégrée | comparaison **exacte** : casse à la moindre différence d'anticrénelage SIMD (x86 / arm) |
| B. `dssim-core` 3.5.1 (SSIM perceptuel) | mesure fiable | **AGPL-3.0** [2] : à éviter même en dépendance de test |
| C. **comparateur à tolérance** (`image-compare` 0.5.0, MIT, ou ~50 lignes maison) | seuils explicites, image de différence | à écrire et régler |

**Choix : C.** Chaque cas = un `document.json` + un PNG de référence. Échec si un canal dépasse **Δ 3/255** ou si plus
de **0,1 %** des pixels diffèrent (seuils à régler sur les premiers cas). En échec, la CI publie référence, résultat
et différence en artefact. Conditions de déterminisme : polices de test libres (OFL) livrées dans `tests/fonts/`,
polices système **désactivées** dans `fontdb`, échelle et taille fixées. Couverture : chaque outil (flèche, cadre,
ellipse, texte FR avec accents, crayon, flou, pixellisation, masque, compteur, surligneur), HiDPI ×2, document sur
grande image (5K) pour la durée. Lancés sur Linux x86_64 **et** macOS arm64 : un écart entre les deux est un bogue,
puisque le rendu se veut identique au pixel près (architecture §3). Régénération seulement par commande explicite,
avec revue visuelle du diff.

### 1.4 Envoi contre de vrais serveurs

Options : A. faux serveurs seuls (`wiremock`) : rapides, mais ne prouvent rien sur Zipline, S3 ou SSH réels ;
B. `testcontainers` 0.28 depuis les tests : autonome, mais lourd et moins facile à déboguer ; C. **`compose.yaml` de
test** lancé par un script, tests marqués `it` : le même fichier sert au développeur à la main.

**Choix : C pour les serveurs réels, A pour le HTTP générique.** Images épinglées par digest :

- **Zipline 4.8.3** (+ PostgreSQL) : envoi simple, en-têtes `x-zipline-*`, **morceaux** au-delà de `chunks.size`
  (abaissé à 1 Mo dans le test), jeton partiel expiré, redémarrage du serveur en cours d'envoi (état perdu : on doit
  repartir de zéro, uploads §1.1).
- **Garage v2.5.0** comme cible S3 principale (maintenue) ; **MinIO** en dernière image publiée seulement, puisque
  l'édition communautaire est archivée (uploads §1.2). Multipart, reprise après arrêt du processus (`UploadId` + ETag),
  style « path », URL présignée.
- **OpenSSH** (`linuxserver/openssh-server` ou `atmoz/sftp:alpine`, mis à jour le 06/11/2026) : clé et mot de passe,
  `known_hosts` strict, **changement de clé d'hôte refusé**, reprise à l'offset de `nom.part`.
- **`wiremock` 0.6.5** en processus : destinations TOML et `.sxcu` importés, statuts d'erreur, délai dépassé, corps de
  plus de 1 Mio refusé, redirection cross-origin refusée, URL `javascript:` refusée avant copie.

Ces tests tournent **sous Linux seulement** : les runners macOS hébergés n'ont pas Docker. Le code d'envoi étant
commun, c'est acceptable ; seul le trousseau macOS a son propre test (§1.6).

### 1.5 CLI

`trycmd` (fichiers `tests/cmd/*.toml` : arguments, sortie attendue, code de sortie) pour `--help`, erreurs et
`--json` ; `assert_cmd` pour les scénarios avec serveur (`kropa dest import x.sxcu`, `kropa dest test`,
`kropa queue retry`). Les **codes de sortie** et le **schéma JSON** sont une API : un changement non voulu casse un
instantané (Marc, product §1). Le jeton passé en argument doit être refusé (product §2, « jamais en argument »).

### 1.6 Sans écran sous Linux, et trousseaux

| Option | Ce qu'elle prouve | Coût |
|---|---|---|
| A. **`xdg-desktop-portal` réel + backend factice** (fonctionnalité `backend` d'`ashpd` 0.14, ou `python-dbusmock`, méthode des tests du portail lui-même [3]) | notre client de portail : demande, refus, annulation, jeton de restauration, image renvoyée = fixture | faible, `dbus-run-session`, à chaque PR |
| B. **Sway headless** (`WLR_BACKENDS=headless`) + `xdg-desktop-portal-wlr` | back-end wlroots réel (screencopy), overlay layer-shell qui s'ouvre et se ferme, capture d'une image connue | moyen, **hebdomadaire** |
| C. `mutter --headless --virtual-monitor` [4] | coquille GTK sous un vrai Mutter (fenêtres, focus, presse-papiers avec focus) | moyen ; le portail Screenshot GNOME passe par GNOME Shell, pas par Mutter seul : **non couvert** |

**Choix : A à chaque PR, B et C (tests de fumée GTK : ouverture, fermeture, raccourcis internes) chaque semaine.**
Trousseau : `gnome-keyring-daemon` déverrouillé dans la session D-Bus de test (Secret Service réel) ; sous macOS,
trousseau temporaire créé par `security create-keychain` dans le job. Le chemin GNOME/KDE réel (dialogues de
permission, GlobalShortcuts, multi-écran) **ne s'automatise pas** honnêtement : il va à la check-list.

### 1.7 Ce qui ne se teste qu'à la main

Sur machine réelle, avant chaque version mineure (≈ 1 h par plateforme), résultat consigné dans `docs/compat.md` :

**GNOME Wayland** (Fedora et Ubuntu à jour ; Flatpak puis `.deb`/`.rpm`)
- [ ] Dialogue de permission expliqué avant ; refus puis « Réinitialiser les autorisations ».
- [ ] Zone, plein écran, écran actif, multi-écran à **échelles mixtes** (100 % + 200 %) ; capture différée.
- [ ] Raccourci par le portail GlobalShortcuts ; repli « raccourci du bureau → `kropa capture area` ».
- [ ] Image copiée avec le focus de l'overlay ; « Lien prêt — Copier » après envoi (jamais de faux « copié »).
- [ ] Notification avec actions (pas de miniature) ; glisser-déposer vers Nautilus et un navigateur.
- [ ] Trousseau (gnome-keyring / oo7, portail Secret en Flatpak) ; repli `age` sans trousseau.

**KDE Plasma Wayland** : mêmes points ; overlay layer-shell et ScreenShot2 en natif ; ce que le Flatpak **filtre**
(layer-shell ? question Linux n° 4) ; KWallet. **wlroots et X11** (au mieux) : zone, plein écran, `bindsym`,
presse-papiers.

**macOS 15 et 26 sur Apple silicon** (Intel si un Mac est disponible)
- [ ] `.dmg` téléchargé : « Ouvrir quand même » + mot de passe admin, à l'installation **et** à la mise à jour.
- [ ] Enregistrement de l'écran et Keychain **conservés après mise à jour** avec le certificat auto-signé
  (platform-macos §4.3) ; même essai en ad hoc pour documenter l'écart.
- [ ] Option seule refusée avec message ; multi-écran Retina + externe ; barre de menus, `NSPanel`, glisser vers Finder.
- [ ] Permission retirée en cours de session (rappel mensuel) : erreur claire, pas d'image noire.

Aucun runner hébergé n'accorde TCC à notre application : pas de capture macOS en CI (au mieux, vérifier qu'un refus
donne une erreur claire).

## 2. Intégration continue

Options : **GitHub Actions** (runners macOS arm64 et Intel gratuits pour un dépôt public, Releases et attestations au
même endroit ; dépendance à GitHub), Forgejo + Woodpecker (libre, mais aucun runner macOS), CI auto-hébergée
(maintenance de machines, intenable seul).

**Choix : GitHub Actions.** Runners vérifiés [5] : `ubuntu-24.04`, `ubuntu-26.04`, `ubuntu-24.04-arm` ; `macos-15`,
`macos-26` (arm64, 3 cœurs M1, 7 Go) ; `macos-15-intel`, `macos-26-intel`. `macos-latest` pointe sur `macos-26` depuis
juin 2026 : **ne jamais l'utiliser**, épingler la version. `macos-14` n'est plus listé (annonce de retraite du 01/10/2026) ; l'Intel
macOS disparaît des runners à la retraite de `macos-15`, prévue à l'automne 2027 [6].

### 2.1 Jobs

| Déclencheur | Jobs | Durée visée |
|---|---|---|
| Chaque PR et `main` | `fmt` (`cargo fmt --check`), `clippy` (`-D warnings`, toutes cibles, Linux et macOS), `deny`, tests nextest + doctests sur `ubuntu-24.04` et `macos-15`, rendu (§1.3) sur les deux, portail factice (§1.6 A), MSRV (`cargo +1.95 check --workspace` sur Linux), `zizmor` sur les workflows | **< 12 min** en parallèle, cache chaud |
| Chaque PR touchant `kropa-upload` ou `kropa-cli`, et `main` | intégration envoi (§1.4), `ubuntu-24.04` | < 10 min |
| Hebdomadaire | Sway headless et Mutter headless (§1.6 B, C), `macos-26`, `macos-15-intel`, `ubuntu-24.04-arm`, `ubuntu-26.04`, couverture `cargo-llvm-cov` 0.9.1 (informative, non bloquante), dernière stable Rust (alerte précoce) | < 30 min |
| Étiquette `vX.Y.Z` | §4.3 | < 45 min |

Une PR fusionnable = jobs PR verts. Pas de seuil de couverture bloquant : il pousse à écrire des tests creux.

### 2.2 Dépendances, licences et sécurité de la chaîne

Options : `cargo-audit` 0.23.0 (RustSec seulement) ou **`cargo-deny` 0.20.2** (`advisories` RustSec, `licenses`,
`bans`, `sources` dans une seule config).

**Choix : `cargo-deny` seul**, via `EmbarkStudios/cargo-deny-action` v2.1.1. La « compatibilité GPL-3.0-or-later »
n'est pas déduite par l'outil : on la **traduit en liste blanche** revue une fois — `MIT`, `Apache-2.0`,
`BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0`, `BSL-1.0`, `CC0-1.0`, `Unlicense`,
`LGPL-2.1-or-later`, `GPL-3.0-or-later` ; **refus** de `GPL-2.0-only`, `AGPL-*` (gifski, dssim), `SSPL`, licences
inconnues. Toute exception (`clarify`, `exceptions`) est commentée dans `deny.toml`. Contrôlé le 11/11 : `aws-lc-sys`
0.47.0 (TLS par défaut de reqwest 0.13) n'a plus la licence OpenSSL historique, il passe la liste [7]. `sources` :
crates.io seul, aucun dépôt git. `bans` : doublons signalés en avertissement. Un job planifié chaque
semaine relance l'audit, car un nouvel avis RustSec arrive sans aucune PR.

Autres règles : `Cargo.lock` commité, `--locked` partout ; **notices tierces** générées par `cargo-about` 0.9.3
(`THIRD-PARTY-LICENSES.html` dans chaque paquet : MIT/BSD/Apache exigent la reproduction des avis) ; `cargo machete`
0.9.2 hebdomadaire ; Dependabot (écosystèmes `cargo` et `github-actions`) groupé chaque semaine ; `permissions:
contents: read` par défaut dans chaque workflow ; aucun secret exposé aux PR de forks.

### 2.3 Toolchain, MSRV, cache

- Stable **1.99.1** (13/10/2026) épinglée par `rust-toolchain.toml` ; **MSRV 1.95** vérifiée par un job (politique de
  l'Architecture §7). `cargo-msrv` 0.19.6 seulement pour trouver la nouvelle valeur lors d'une hausse.
- Cache : `Swatinem/rust-cache` v2.9.3 (registre + `target/`), **sauvegardé seulement depuis `main`** (pas
  d'empoisonnement par une PR) ; outils en binaires par `taiki-e/install-action` v2.90.3 ; sccache écarté.

### 2.4 Actions épinglées par SHA (relevées le 11/11/2026)

| Action | Version | SHA |
|---|---|---|
| actions/checkout | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| Swatinem/rust-cache | v2.9.3 | `d1527c8f67ef9af357290475d4af4b0fab8f12e0` |
| dtolnay/rust-toolchain | branche `master` (pas de tags) | `683b588c487a69d01f6c6ddf40f036ff682cc2c8` |
| taiki-e/install-action | v2.90.3 | `cfdb7ed4ccc7c7dce37759eae89dc5674b469b39` |
| EmbarkStudios/cargo-deny-action | v2.1.1 | `3c6349835b2b7b196a839186cb8b78e02f7b5f25` |
| flatpak/flatpak-github-actions | v6.8 | `79327416609af08178ad73b352877e51450790b3` |
| actions/upload-artifact | v7.0.2 | `cf430e030ddbb5b0abf93d22962f4752f3646cd9` |
| actions/attest-build-provenance | v4.2.2 | `4d101475d8b20a2381f78447822ac1eab6504dd8` |
| softprops/action-gh-release | v3.0.4 | `8a0cd651e5b2365bd640854a3fa5dfc70e4875f3` |

Pour l'import du certificat macOS, les commandes `security` documentées par GitHub [8] suffisent : pas d'action tierce
(`Apple-Actions/import-codesign-certs` v7.2.0 existe, mais elle verrait la clé privée).

## 3. Empaquetage et distribution

### 3.1 Linux

| Canal | Permet | Limite |
|---|---|---|
| **Flathub** | mises à jour automatiques, App ID fourni aux portails, canal attendu sur GNOME | rappels de l'équipe Linux : **pas de KWin ScreenShot2** (fenêtre propre sans dialogue sur KDE), layer-shell et screencopy **peut-être filtrés** par security-context (KWin, Sway, Hyprland, niri : à mesurer), glisser-déposer sortant via le portail FileTransfer (non vérifié), Secret Service via le portail Secret (prise en charge par `keyring` à vérifier), Tesseract (OCR v1.3) à embarquer, la commande hôte est `flatpak run --command=kropa <app-id>` (alias à documenter pour les raccourcis du bureau) |
| `.deb` / `.rpm` sur GitHub Releases | KDE et wlroots complets (ScreenShot2, layer-shell), CLI dans le `PATH` | **pas de mises à jour** sans dépôt ; un build par famille (glibc, version de GTK4) |
| COPR (Fedora), OBS (openSUSE, aussi Debian/Ubuntu) | dépôts avec mises à jour, gratuits | une recette par système (`.spec`, `debian/`) à maintenir |
| AppImage (runtime type 2 statique, sans libfuse2 [9]) | un fichier, toutes distributions | **fragile pour les portails** (pas de `.desktop` installé, App ID incertain), GTK4 à embarquer, pas de mises à jour |
| `cargo install --locked` | développeurs, serveurs (CLI seule, sans GTK) | compilation, GUI non promise par ce canal |
| `PKGBUILD` dans le dépôt (Produit) | utilisateurs d'Arch | non publié (pas d'AUR) ; non testé en CI → **à ne pas promettre** |

Outils : **`cargo-deb` 3.8.0** et **`cargo-generate-rpm` 0.21.1** (métadonnées dans `Cargo.toml`, rien d'autre à
apprendre) plutôt que nfpm (Go, un fichier de plus) ou des recettes natives complètes (repoussées à COPR/OBS). Builds
dans des conteneurs `debian:13` et `fedora:<stable>` pour lier la bonne glibc et la bonne GTK. Les `.deb`/`.rpm`
installent `<app-id>.desktop`, l'icône et la métainfo (exigés par les portails, platform-linux §4).

**Flathub, exigences actuelles [10][11]** : ID inversé de 3 à 5 éléments, `io.github.<compte>.<Nom>` pour un projet
GitHub, vérifié par connexion au compte propriétaire ; MetaInfo valide **dans le dépôt amont**, `.desktop`, icône SVG ;
**construction hors ligne** : les crates passent par `flatpak-cargo-generator.py` (`Cargo.lock` →
`cargo-sources.json`, `CARGO_NET_OFFLINE`) ou un tarball `cargo vendor` publié avec la release [12] ; extension
`org.freedesktop.Sdk.Extension.rust-stable` (branche 26.08 en stable : non vérifié) ; runtime GNOME 51 (base Freedesktop SDK 26.08) ; permissions statiques
minimales, portails obligatoires quand ils existent ; versions stables uniquement ; historique de développement
« significatif » exigé ; applications « tray-only » refusées (Kropa a des fenêtres : à défendre). Deux règles pèsent
directement sur ce projet : **le manifeste Flathub ne doit contenir aucun contenu généré ou assisté par IA**, tout code
ou empaquetage généré par IA doit être **déclaré**, et **aucun agent IA ne doit ouvrir ni automatiser la PR de
soumission** [10]. Le manifeste et la soumission seront donc écrits et envoyés **à la main par le développeur**. Enfin,
un renommage après acceptation est accepté « avec prudence » : **soumettre seulement une fois le nouveau nom fixé**.

**Choix Linux** : v1.0 = Flathub + `.deb` + `.rpm` + archive CLI (x86_64 et aarch64). AppImage écarté tant que
personne ne le demande ; COPR/OBS en v1.x si les `.deb`/`.rpm` ont des utilisateurs.

### 3.2 macOS

Options pour le `.app` : `cargo-bundle` 0.12.0 ou `cargo-packager` 0.11.8 (peu actif depuis 11/2025) : contrôle
partiel d'`Info.plist` (`LSUIElement`, CLI dans le bundle) ; `dist` 0.34.0 : **ni `.app` ni `.dmg`** d'après sa doc
[13], formule de binaire nu contraire à platform-macos §4.2 ; **script maison** (~80 lignes : `Info.plist`, `lipo`,
`codesign`, `hdiutil`), explicite et sans outil tiers.
**Choix : script maison.** Étapes : compiler `aarch64-apple-darwin` et `x86_64-apple-darwin` sur `macos-15` (arm64),
`lipo -create` pour `kropa` et `kropa-app`, bundle `Kropa.app` (CLI dans `Contents/MacOS/`, lien proposé vers le
`PATH`), signature, `hdiutil create -format UDZO` (pas de `create-dmg`). Le binaire Intel est **exécuté** dans le job de
release sur `macos-15-intel` (tests du cœur) avant publication.

**Signature** — options : ad hoc (`codesign -s -`) : rien à protéger, mais permissions TCC et Keychain **perdues à
chaque mise à jour** ; **certificat auto-signé stable** : DR `certificate leaf = H"…"` stable, permissions conservées
(sources non Apple [14], **à vérifier sur machine**), mais clé à protéger : sa fuite permettrait de signer un binaire
qui hérite des permissions de Kropa. **Choix : certificat auto-signé**, conformément à platform-macos §4.3 :
- créé **une fois** hors CI (Trousseau ou `openssl`, usage étendu `codeSigning`, validité 20 ans : un changement de
  certificat fait perdre les permissions à tous) ; copie de secours chiffrée par `age` hors ligne ;
- `.p12` en base64 + mot de passe + mot de passe de trousseau temporaire stockés comme **secrets d'environnement**
  GitHub (`release`), pas de dépôt ; environnement limité aux étiquettes `v*` avec **approbation manuelle** du
  mainteneur ; job de signature séparé, sans cache, sans action tierce ; trousseau temporaire créé, déverrouillé,
  `set-key-partition-list`, détruit en fin de job [8] ;
- `codesign --force --timestamp=none --sign "<identité>"` du binaire intérieur au bundle ; runtime renforcé
  (`--options runtime`) **seulement après test** (inutile sans notarisation, non vérifié) ; `codesign --verify
  --strict` et affichage de la DR dans le journal de release pour détecter tout changement d'identité.

**Homebrew** : cask officiel impossible (décisions). Tap personnel `homebrew-tap` (`brew tap-new`) avec **formule
compilée depuis les sources** (`depends_on "rust" => :build`) : pas de quarantaine, mais compilation longue, signature
ad hoc locale (permissions redemandées à chaque `brew upgrade`, sauf script de certificat local à la yabai), confiance
du tap à donner explicitement (`brew trust`) [15]. Homebrew 7.0 exécute les constructions **en bac à sable** et migre
vers des installations **sans réseau** après une phase de téléchargement [16] : prévoir le tarball `cargo vendor` comme
ressource de la formule plutôt qu'un `cargo fetch` pendant la construction. Intel en Tier 3 chez Homebrew : compilation
locale de toute façon. **Choix : tap en v1.x**, après validation sur machine réelle du `.dmg` et de la formule.

### 3.3 Ordre proposé

| Jalon | Canaux |
|---|---|
| v0.1 – v0.4 | GitHub Releases en « pré-version » : archives CLI Linux/macOS, `.deb`, `.rpm`, `.dmg` (dès que la GUI existe) ; `cargo install --locked --git … --tag` |
| **v1.0** | + **Flathub** (soumission manuelle), `.dmg` universel signé auto-signé, page « signature et ses conséquences » |
| v1.x | tap Homebrew compilé ; publication crates.io (`release-plz` 0.4.3 ou `cargo publish` des 11 crates, à décider) ; COPR/OBS si demande |
| Sur demande | AppImage, `PKGBUILD` testé en CI dans un conteneur Arch |

## 4. Publication

### 4.1 Versionnement et changelog

- **SemVer** : `0.x` jusqu'à la v1.0 (rupture possible à chaque mineure), puis API stable = CLI (sous-commandes, codes
  de sortie, `--json`), format `.kropa`, `config.toml` et destinations TOML. Une seule version pour tout le workspace.
- Commits **Conventional Commits** (`feat:`, `fix:`, `feat!:`) vérifiés en CI sur le titre de PR (fusion squash).
- Changelog — options : `release-plz` (PR de release automatique, publication crates.io, changelog via git-cliff) ;
  **`git-cliff` 2.14.2 seul** ; changelog entièrement manuel. **Choix : `git-cliff` produit le brouillon, le
  développeur le réécrit pour les utilisateurs** dans `CHANGELOG.md` (format Keep a Changelog, FR et EN). release-plz
  reviendra avec la publication crates.io.
- Les notes de version de GitHub Releases reprennent la section du changelog et une rubrique fixe « **Limites
  connues** » (renvoi à la matrice de compatibilité). La balise `<releases>` de la MetaInfo est générée depuis le
  changelog (Flathub et les logithèques l'affichent).

### 4.2 Mises à jour, local d'abord

Options : vérification automatique (GitHub API) : **requête non configurée**, interdite par le brief ; ou **rien par
défaut**, canaux gérés + vérification explicite : les utilisateurs de `.dmg`/`.deb` ne sont prévenus que s'ils le
demandent.
**Choix** : aucun appel réseau de mise à jour par défaut. Flathub (et plus tard Homebrew, COPR) mettent à jour.
`kropa version --check` interroge GitHub **à la demande** ; un réglage « vérifier au démarrage », désactivé par défaut,
fait la même chose. Pas de téléchargement-installation intégré en v1 (macOS : la quarantaine reste ; question de
platform-macos §4.5 laissée ouverte).

### 4.3 Procédure de release

1. Branche `main` verte, jobs hebdomadaires verts (Intel, macOS 26, Sway).
2. Check-list manuelle §1.7 pour une mineure ; `docs/compat.md` mis à jour.
3. Version dans `Cargo.toml`, `CHANGELOG.md`, MetaInfo ; commit `chore(release): vX.Y.Z` ; étiquette **signée** `vX.Y.Z`.
4. Workflow d'étiquette : builds Linux (conteneurs) et macOS, tests rapides sur chaque binaire produit, job `release`
   approuvé à la main (signature macOS), `SHA256SUMS`, **attestations de provenance** (`gh attestation verify`), tarball
   `cargo vendor` pour Flatpak et Homebrew, brouillon de GitHub Release.
5. Installation du `.dmg` et d'un paquet depuis le brouillon sur machine réelle, puis publication ; manifeste
   Flathub mis à jour **à la main** (plus tard, formule du tap).

## 5. Vidéo Linux : GStreamer ou FFmpeg — point clos

La vidéo et le GIF sont retirés : **ni GStreamer, ni FFmpeg, ni `imagequant`/`gif`** ne sont nécessaires. Il reste un
seul besoin multimédia, et **pas en v1** : un flux d'écran pour la capture défilante (classée W par Produit) et,
éventuellement, la fenêtre « propre » sur GNOME par ScreenCast `WINDOW` (v1.2 ; le portail Screenshot `target` peut
suffire, non vérifié). Options pour lire ce flux : GStreamer `pipewiresrc` (pipeline complet, greffons variables selon
la distribution) ou **`pipewire` 0.10.1 directement** (libpipewire, MIT ; quelques images à recadrer, aucun encodeur).
**Choix : `pipewire` direct**, en négociant des tampons mémoire (MemFd/SHM) pour éviter dmabuf et EGL ; dépendance
système `libpipewire-0.3`, présente sur tous les bureaux visés. Sous macOS, ScreenCaptureKit suffit. Conséquences :
pas d'extension `codecs-extra` ni de `--socket=pulseaudio` dans le Flatpak ; la commande FFmpeg de nettoyage des
métadonnées vidéo (uploads §8) disparaît.

## 6. Désaccords ou points à transmettre

- **Architecture** : la justification de la GPL-3.0-or-later reposait en partie sur FFmpeg, x264 et `imagequant` ; elle
  tient encore (dépendances Apache-2.0, choix d'un projet libre), mais le point mérite d'être réécrit. Retirer
  `kropa-video` des 11 crates. Prévoir un **constructeur unique de client HTTP** (test « zéro requête »), une **horloge
  injectable** et l'option de désactiver les polices système dans `kropa-render` (tests).
- **Désaccord Architecture / Envoi** : S3 par `reqwest` + `aws-sigv4` (architecture §6) contre `object_store` 0.14.2
  (uploads §0). Les tests §1.4 valent pour les deux, mais la liste de licences et l'arbre de dépendances en dépendent :
  à trancher.
- **Produit** : « PKGBUILD dans le dépôt » et « Homebrew tap » en v1.0 ; je propose le tap en v1.x et un PKGBUILD
  non promis tant qu'il n'est pas testé. Flathub exige un historique de développement : la soumission ne peut pas
  arriver le jour de la première version publique.
- **Flathub et IA** (§3.1) : à connaître **avant** de faire de Flathub le canal principal GNOME.
- **macOS** : le certificat auto-signé est une clé à fort pouvoir (permissions héritées) ; sauvegarde, rotation
  impossible sans coût pour tous les utilisateurs. Runtime renforcé et invites Keychain à tester sur machine.
- **Linux** : mesurer d'abord le filtrage Flatpak de layer-shell/screencopy (question n° 4) : il décide du canal KDE.
- **Nom** : App ID, crate, tap, bundle ID et identité de signature dépendent du nom définitif. Rien n'est publié sous
  un identifiant durable avant ce choix (le certificat porte un nom commun : le créer **après** le renommage).

## 7. Questions ouvertes

1. Quel Mac réel (Apple silicon, et Intel ?) pour la check-list macOS ? Sans Intel, le binaire universel n'est vérifié
   que sur runner.
2. Accepte-t-on la déclaration d'usage d'IA exigée par Flathub, ou Flathub sort-il des canaux (Flatpak en bundle seul,
   sans mises à jour) ?
3. Publication sur crates.io des 11 crates (pour `cargo install kropa`) ou seulement `cargo install --git` ?
4. Versions minimales Linux visées (Ubuntu 24.04 LTS ? Debian 13 ?) : elles fixent le conteneur de build `.deb`.
5. Seuils de la comparaison d'images (Δ 3/255, 0,1 %) à confirmer sur les premiers rendus réels Linux/macOS.
6. Approbation manuelle de chaque release (environnement GitHub) acceptée, au prix d'un clic par version ?

## Sources (consultées le 11/11/2026)

Versions des crates : API `https://crates.io/api/v1/crates/<nom>` (cargo-nextest 0.9.151, insta 1.49.0, assert_cmd
2.2.2, trycmd 1.2.1, proptest 1.12.0, image-compare 0.5.0, dssim-core 3.5.1, testcontainers 0.28.0, wiremock 0.6.5,
cargo-deny 0.20.2, cargo-audit 0.23.0, cargo-llvm-cov 0.9.1, cargo-about 0.9.3, cargo-machete 0.9.2, cargo-msrv 0.19.6,
cargo-deb 3.8.0, cargo-generate-rpm 0.21.1, cargo-bundle 0.12.0, cargo-packager 0.11.8, cargo-dist 0.34.0, git-cliff
2.14.2, release-plz 0.4.3, zizmor 1.32.0, pipewire 0.10.1, ashpd 0.14.0, aws-lc-sys 0.47.0). Actions : `git ls-remote`
sur chaque dépôt. Rust : https://static.rust-lang.org/dist/channel-rust-stable.toml. Images : Docker Hub
(`diced/zipline` 4.8.3, `dxflrs/garage` v2.5.0, `atmoz/sftp`, `linuxserver/openssh-server`).

1. https://nexte.st/ · 2. https://crates.io/crates/dssim-core (AGPL-3.0)
3. https://conf.linuxappsummit.org/event/7/contributions/218/attachments/47/81/las-testing-with-portals.pdf ·
   https://sources.debian.org/src/glib2.0/2.85.1-2/gio/tests/memory-monitor-portal.py.in
4. https://pygobject.gnome.org/guide/testing.html · https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/1698
5. https://docs.github.com/en/actions/reference/runners/github-hosted-runners
6. https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/ ·
   https://github.blog/changelog/2026-10-01-github-actions-macos-14-runner-image-retirement ·
   https://bugs.openjdk.org/browse/JDK-8377375
7. https://crates.io/crates/aws-lc-sys · https://embarkstudios.github.io/cargo-deny/
8. https://docs.github.com/en/actions/how-tos/deploy/deploy-to-third-party-platforms/sign-xcode-applications
9. https://github.com/AppImage/type2-runtime ·
   https://discussion.fedoraproject.org/t/f44-change-proposal-atomic-desktops-drop-fuse-2-libraries-selfcontained/179410
10. https://docs.flathub.org/docs/for-app-authors/requirements
11. https://docs.flathub.org/docs/for-app-authors/verification · https://thisweek.gnome.org/posts/2026/08/twig-263/
12. https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo · 13. https://axodotdev.github.io/cargo-dist/book/
14. https://developer.apple.com/forums/thread/795739 · https://nickboy-github-io.pages.dev/posts/tcc-cdhash-trap/ ·
    https://raw.githubusercontent.com/jackielii/skhd.zig/main/docs/CODE_SIGNING.md
15. https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap · 16. https://www.helpnetsecurity.com/2026/09/15/homebrew-7-0-0-security-open-source/

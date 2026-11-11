# Tests manuels macOS : capture par ScreenCaptureKit (lot 1.5)

> Ce que la CI ne peut pas prouver. Le runner `macos-15` n'a ni la permission « Enregistrement de l'écran » (TCC),
> ni l'assurance d'une session ouverte, ni plusieurs écrans. La CI construit `Kropa.app` (universel, signature *ad hoc*),
> vérifie la signature et l'architecture, lance `kropa doctor` depuis le bundle (il doit se reconnaître : `app_bundle`
> et `bundle_identifier`), et exécute les tests qui n'ont pas besoin de la permission : logique des écrans et de la
> permission (`apple.rs`, `screens.rs`), `tests/macos.rs`, `tests/cmd-macos/`. Elle ne dit **rien** de la capture réelle,
> de TCC, des boîtes de dialogue ni du multi-écran : c'est l'objet de cette liste, à faire sur le Mac de l'utilisateur.
> Tout ce qui est coché ici l'est à la main, et le résultat se note à la fin.

## Avant de commencer

- [ ] Noter : version de macOS, architecture (Apple silicon ou Intel), nombre d'écrans.
- [ ] Construire le bundle : `scripts/bundle-macos.sh` (release, signature ad hoc, sortie `target/bundle/Kropa.app`).
  La fin de la sortie donne la **designated requirement** : noter si elle contient `cdhash` (attendu en ad hoc).
  Option rapide : `--profile dev`. Binaire universel : `--universal` (après `rustup target add aarch64-apple-darwin
  x86_64-apple-darwin`), puis `lipo -archs target/bundle/Kropa.app/Contents/MacOS/kropa` doit afficher les deux.
- [ ] `codesign --verify --strict --verbose=2 target/bundle/Kropa.app` : code 0.
- [ ] Un bundle construit sur la machine n'a pas d'attribut de quarantaine (`xattr -l target/bundle/Kropa.app` : vide).
  Pour un bundle téléchargé : noter ce que macOS demande à l'ouverture (« Ouvrir quand même », mot de passe).
- [ ] Mettre `kropa` dans le PATH, vers le binaire **du bundle** :
  `ln -sf "$PWD/target/bundle/Kropa.app/Contents/MacOS/kropa" ~/.local/bin/kropa`.
- [ ] `kropa doctor` (puis `--lang fr`, `--lang en`, `--json`) : code 0. Relever :
  version de macOS ; session ouverte « oui » ; permission d'enregistrement de l'écran ; `app_bundle` = chemin du `.app` ;
  `bundle_identifier` = `io.github.fs0ciety7000.Kropa` ; chaque écran avec son **nom** et son **identifiant**, le principal
  marqué ; plus de ligne « Écrans et leurs noms : lot 2.5 » dans « Pas encore disponible ».
- [ ] Même commande avec le binaire nu (`target/release/kropa doctor`) : `app_bundle` « introuvable ».

## Permission « Enregistrement de l'écran » (une seule à la fois)

Le panneau s'appelle Réglages Système › Confidentialité et sécurité › **Enregistrement de l'écran et de l'audio système**
(macOS 15 et 26 ; noter le nom exact vu sur la machine : les messages de Kropa le citent).

**État de départ.** Retirer l'entrée de Kropa (bouton « − ») ; `tccutil reset ScreenCapture io.github.fs0ciety7000.Kropa`
; et, si on lance depuis un terminal, `tccutil reset ScreenCapture com.apple.Terminal` (ou l'identifiant de iTerm2, etc.).

- [ ] `kropa doctor` : permission « non », `capture screen` et `capture output` « indisponible », raison avec le chemin dans
  les Réglages.
- [ ] `kropa capture screen --output /tmp/k.png` : une boîte du système apparaît-elle ? **Noter son texte exact** (et si
  c'est celui de `NSScreenCaptureUsageDescription`, en français ou en anglais selon la langue du système : la clé n'est
  pas documentée par Apple, hypothèse à confirmer) et **quelle application elle nomme : Kropa ou le terminal**.
- [ ] Même commande : **code 4**, `error.reason` = `screen_recording_denied`, message qui donne le chemin des Réglages,
  stdout vide, **aucun fichier** créé. Avec `--json` : un seul document sur stdout.
- [ ] `kropa capture screen --delay 15 --output /tmp/k.png` : l'erreur arrive **tout de suite**, pas après 15 s.
- [ ] Relancer sans rien changer : la boîte du système revient-elle, ou le refus est-il silencieux ? (noter ; le code reste 4.)

**Accorder.**

- [ ] Ouvrir le panneau : quelle entrée est listée, **Kropa** ou **Terminal** (ou iTerm2) ? C'est la question de fond de
  ce lot : macOS attribue la permission à l'application *responsable* du processus. Lancé depuis un terminal, il se
  peut que ce soit le terminal. Noter le résultat pour : terminal → `kropa` (lien symbolique) ; terminal → chemin direct
  du `.app` ; `open -a target/bundle/Kropa.app --args capture screen --output /tmp/k.png` (Launch Services).
- [ ] Activer l'interrupteur. macOS propose-t-il « Quitter et rouvrir » ? Faut-il rouvrir le terminal ? Noter.
- [ ] `kropa capture screen --output /tmp/k.png` : **code 0**, chemin absolu sur stdout, `Enregistré : … (LxH)` sur stderr.
- [ ] Ouvrir l'image : tous les écrans, **résolution native** (ex. 3024×1964 sur un MacBook Pro 14″, pas la moitié), **sans
  curseur**, couleurs justes. `sips -g all /tmp/k.png` : pas de date, de logiciel ni de texte ; noter le profil (`sips -g
  profile`, attendu : celui de l'écran, ex. Display P3) et la résolution en ppp.
- [ ] `kropa --json capture screen --output /tmp/k2.png` : `capture.backend` = `screencapturekit`, `capture.scale` = 2.0
  sur Retina, `file.width`/`file.height` = ceux de l'image.
- [ ] macOS 15 : la boîte « autoriser pour un mois » (rappel périodique) : si elle apparaît pendant la capture, la
  commande attend la réponse (60 s au plus) ; noter son aspect et si le code de sortie est juste après un délai dépassé
  (`screencapturekit_no_response`).

**Retrait et entrée périmée.**

- [ ] Désactiver l'interrupteur, relancer : code 4 `screen_recording_denied`.
- [ ] Après un nouveau build ad hoc (modifier une ligne, `scripts/bundle-macos.sh`) alors que l'ancienne entrée est
  encore cochée : que dit `kropa doctor` (permission « oui » ?) et que rend `capture screen` ? Attendu si macOS refuse :
  code 4 `screen_recording_stale` avec la marche « retirer puis rajouter ». Si on obtient `screen_recording_denied` ou
  un autre code, le noter : l'hypothèse « préflight vrai mais capture refusée » est **non vérifiée**.

## Écrans

- [ ] `kropa capture output` sans nom : l'écran **principal** (celui de la barre des menus) ; JSON `capture.output` =
  son nom.
- [ ] `kropa capture output "<nom relevé par doctor>"` (casse libre) puis par **identifiant** numérique : même image.
- [ ] `kropa capture output "inconnu"` : **code 10** `output_not_found`, immédiat, **sans** demande de permission (le
  vérifier avec la permission retirée : le code doit rester 10, pas 4).
- [ ] Deux écrans de même modèle (même nom) : `capture output "<nom>"` → code 10 `output_not_found`, le message liste les
  identifiants à utiliser.
- [ ] Deux écrans, même échelle : `capture screen` = largeur cumulée, positions relatives justes.
- [ ] Deux écrans d'échelles différentes (Retina + 1×) et/ou **décalés en hauteur** : image assemblée à l'échelle du plus
  fin, position correcte, bandes **transparentes** là où il n'y a pas d'écran, netteté du Retina conservée. Noter la
  taille de l'image et le temps pris.
- [ ] Écran en miroir, écran éteint, Mac en économiseur d'écran : noter ce qui se passe (image noire, erreur ?).

## Session

- [ ] `ssh localhost kropa capture screen` (connexion à distance activée) : code 10, `no_graphical_session`, message qui
  dit de lancer kropa depuis un terminal du Mac ; pas de plantage ni de blocage.
- [ ] `ssh localhost kropa doctor` : code 0, « session : non », capacités « indisponible » (raison : pas de session).
- [ ] Depuis un raccourci (Automator, Raccourcis) ou un lanceur : même résultat qu'au terminal ?

## Test de survie de la permission (pour la tâche 1.6)

À faire quand l'utilisateur a créé le certificat auto-signé (la création et la garde de la clé ne sont pas faites par le
script ni par l'assistant). `<CERT>` = nom du certificat dans le trousseau.

- [ ] Build 1 : `scripts/bundle-macos.sh --sign "<CERT>"`. Noter la **designated requirement** (attendu :
  `identifier "io.github.fs0ciety7000.Kropa" and certificate leaf = H"…"`, **sans** `cdhash`). macOS demande peut-être
  l'accès à la clé de signature : c'est l'invite du Trousseau, à accepter à la main.
- [ ] Accorder la permission à ce build (voir plus haut) ; `capture screen` : code 0.
- [ ] Build 2 : modifier le code (une chaîne), `scripts/bundle-macos.sh --sign "<CERT>"` **au même emplacement**.
  `codesign -dr - target/bundle/Kropa.app` : la requirement est la même qu'au build 1.
- [ ] **Sans toucher aux Réglages**, `kropa capture screen` : code 0, aucune nouvelle demande. C'est le résultat attendu
  par `platform-macos.md` § 4.3 ; s'il échoue, la distribution macOS est à revoir (plan, risque 1).
- [ ] Build 3 : même chose après `cargo clean`, puis avec le bundle **déplacé** (`/Applications`). Noter.
- [ ] Même séquence en ad hoc (`scripts/bundle-macos.sh`) : le build 2 doit redemander la permission. Noter si l'ancienne
  entrée reste cochée (cas « entrée périmée » ci-dessus).
- [ ] Noter le résultat dans `docs/decisions.md` (tâche 1.6).

## Résultats

À remplir : date, macOS, architecture, terminal utilisé, entrée listée par les Réglages (Kropa ou terminal), cases non
cochées et pourquoi.

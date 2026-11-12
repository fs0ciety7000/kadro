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

## Secrets : Keychain et courtier (lot 1.8m)

> La CI `macos-15` prouve sans certificat : l'élément unique (clavier de test créé par l'API), le verrou, le protocole,
> `LOCAL_PEERTOKEN` sur une `socketpair`, les exigences de code acceptées par le système, le refus du courtier réel par
> un appelant non signé du projet. Elle ne dit **rien** de : l'invite Keychain réelle, un pair signé du certificat, le
> runtime renforcé réel, « Refuser ». C'est cette liste. Prérequis : le certificat auto-signé de la tâche 1.6 (création
> et garde de la clé : l'utilisateur) ; `<CERT>` = son nom.

**Construire.** `scripts/bundle-macos.sh --sign "<CERT>" --hardened-runtime` (les deux binaires sont signés du même
certificat, `kropa-app` à part).

- [ ] `codesign -dv --verbose=4 target/bundle/Kropa.app/Contents/MacOS/kropa-app` : `Identifier=io.github.fs0ciety7000.Kropa.app`,
  `flags=0x10000(runtime)`. Même chose pour le bundle (`Identifier=io.github.fs0ciety7000.Kropa`, `runtime`).
  `codesign -d --entitlements - …` : **aucun** `get-task-allow`.
- [ ] `codesign -dr - …/kropa-app` et `…/Kropa.app` : `certificate leaf = H"…"` **identique** aux deux, sans `cdhash`.

**Une seule invite par mise à jour (le point de U1).** Mettre `kropa` du bundle dans le PATH. Retirer l'élément s'il existe :
Accès au trousseau, chercher `io.github.fs0ciety7000.Kropa`, supprimer.

- [ ] Avec un profil `zipline` à un secret et un profil à **deux** secrets : `kropa secrets set zipline token` puis
  `kropa secrets set s3 access`, `kropa secrets set s3 secret`. Accès au trousseau : **un seul** élément
  `io.github.fs0ciety7000.Kropa`, compte `store` (et non trois). Noter ce que macOS a demandé à chaque commande
  (la création par `kropa` ne doit rien demander : il est le créateur).
- [ ] `kropa secrets status` : `store: keychain (available)`, `zipline: 1`, `s3: 2`, aucune valeur.
- [ ] `kropa upload …` vers une destination à secret : **aucune invite** (même binaire, même `cdhash`).
- [ ] **Build 2** : modifier une chaîne, `scripts/bundle-macos.sh --sign "<CERT>" --hardened-runtime` au même endroit.
  `kropa secrets status` : **une invite** « Kropa veut utiliser… » nommant `kropa` ; choisir « Toujours autoriser ».
  Puis `kropa secrets status` et `kropa upload` : **aucune** invite. Noter si l'invite revient pour les 3 secrets (elle ne
  doit pas : un seul élément). Si macOS en montre plusieurs, noter combien et pourquoi (décision U1 à revoir).
- [ ] Contrôle d'accès de l'élément (Accès au trousseau, onglet « Contrôle d'accès ») : `kropa` listé ; `kropa-app` **absent**.
  `kropa-app` ne lit jamais le Keychain.

**Refus de l'invite.**

- [ ] Après un build 3, `kropa secrets status` et **Refuser** à l'invite : code **4**, `error.reason` ou état
  `keychain_denied`/`denied`, message avec le chemin dans Accès au trousseau, aucun repli vers un autre magasin. **Noter
  l'`OSStatus` exact** (`kropa --json secrets status`, le champ `detail` ne cite que « refused ») : le code lit
  `errSecUserCanceled` (-128) ou `errSecAuthFailed` (-25293) comme un refus, hypothèse à confirmer.
- [ ] Trousseau de session verrouillé (`security lock-keychain login.keychain-db`), sans terminal (`kropa … </dev/null`
  depuis `ssh localhost`) : code **7** `keyring_locked` en moins de 6 s, **aucune** boîte de dialogue.
- [ ] Même commande avec un terminal : l'invite de déverrouillage du système s'affiche ; refuser → `keyring_locked`.

**Courtier : appelant autorisé.** `…/Kropa.app/Contents/MacOS/kropa-app --keychain-status`

- [ ] Sortie `store: keychain-broker`, `state: available`, les mêmes profils et nombres que `kropa secrets status`, code 0.
  **Invite** : combien, et nomme-t-elle `kropa` (le courtier), pas `kropa-app` ? Une seule après une mise à jour, jamais
  deux (CLI puis courtier). Noter.
- [ ] `log stream --predicate 'subsystem == "com.apple.securityd"'` pendant la commande : requêtes présentes (le
  courtier a bien lu le Keychain).

**Courtier : appelants refusés (zéro requête à `securityd`).** Pour chaque cas, lancer `log stream --predicate 'subsystem == "com.apple.securityd"'` dans un
autre terminal et vérifier **qu'aucune requête n'apparaît** pendant la commande.

- [ ] `…/Kropa.app/Contents/MacOS/kropa __store-broker` depuis le Terminal : code **71**, `keychain broker refused: no_channel`
  sur stderr, rien sur stdout.
- [ ] Fd 3 forgé par un shell, `socketpair` de Python lancée par `python3` : `python3 -c "import socket,os,sys;
  a,b=socket.socketpair();os.dup2(b.fileno(),3);os.execv('/…/Kropa.app/Contents/MacOS/kropa',['kropa','__store-broker'])"`
  (le fd est fermé à l'`exec` si CLOEXEC : essayer aussi `os.set_inheritable(3, True)`) : code **75**
  `peer_code_rejected` (le pair est `python3`, pas `kropa-app`), sans requête `securityd`.
- [ ] Parent signé autrement : copier `kropa-app`, le re-signer **ad hoc** (`codesign --force --sign - copie`) et le lancer
  en `--keychain-status` avec un `kropa` voisin copié aussi : `keychain_broker_refused` (75) ou `…_unavailable` (74 :
  le broker lui-même n'est plus signé du certificat). Noter le code.
- [ ] Parent signé du certificat **sans** runtime renforcé (build sans `--hardened-runtime` du seul `kropa-app`) :
  refus 75 (`le pair n'est pas signé avec le runtime renforcé`).
- [ ] Bundle signé **ad hoc** (`scripts/bundle-macos.sh`) : `kropa-app --keychain-status` donne
  `keychain_broker_unavailable` (74) ; `kropa secrets status` (direct) fonctionne.
- [ ] **Jeton périmé** (point 7 de § 2.2.1) : un programme qui crée la `socketpair`, lance `kropa __store-broker` puis fait
  `exec` de `kropa-app` : le courtier doit refuser (`SecCodeCopyGuestWithAttributes` échoue sur le jeton). **S'il
  accepte, le noter dans `decisions.md`** : le cas relève alors du § 7.
- [ ] Processus parent mort avant la réponse : lancer `kropa-app --keychain-status` et le tuer (`kill -9`) pendant l'invite :
  aucune écriture, aucun processus `kropa __store-broker` orphelin après 120 s (`pgrep -fl __store-broker`).

**Concurrence.**

- [ ] Deux `kropa secrets set` simultanés (deux terminaux) sur deux secrets : `kropa secrets status` en voit les deux.
  Dossier `~/Library/Application Support/io.github.fs0ciety7000.Kropa/` en `0700`, `store.lock` en `0600`.

## Presse-papiers : `NSPasteboard` (lot 1.10)

Aucune permission à accorder. Le bundle et le binaire nu doivent donner le même résultat. Le contenu doit rester collable
**après la fin** de `kropa` (le serveur de presse-papiers le garde).

- [ ] Image : `kropa capture screen --output /tmp/k.png --copy image` (permission d'enregistrement de l'écran requise,
  voir plus haut). Code 0, stderr `Image copied` / `Image copiée`. Avec `--json` : `clipboard.status` = `copied`, pas de `reason`.
- [ ] Coller l'image dans **Aperçu** (Fichier > Nouveau depuis le presse-papiers), dans **Notes** et dans **Messages** ou un
  champ de mail : elle apparaît à sa taille native, sans couleurs décalées. Noter la taille affichée (L x H attendus).
- [ ] Texte brut : `kropa upload <FICHIER> --to <PROFIL> --copy url` (ou `markdown`). Coller dans **TextEdit** en texte
  brut (Format > Convertir au format texte) et dans le champ d'adresse de Safari : le lien exact, sans espace ni retour
  à la ligne en trop. Le lien reste aussi sur stdout.
- [ ] HTML : même commande avec `--copy html`. Coller dans **TextEdit** en texte enrichi ou **Notes** : un lien cliquable.
  Coller dans un champ texte brut (Terminal, `pbpaste`) : le balisage `<a href=...>` tel quel.
- [ ] `pbpaste` après chaque copie texte : la valeur exacte. `pbpaste -Prefer public.html` après `--copy html` : le HTML.
- [ ] Remplacement : copier une image, puis un lien : coller dans Aperçu (Nouveau depuis le presse-papiers) ne propose plus
  l'image. Copier un lien, puis une image : `pbpaste` ne rend plus le lien.
- [ ] Depuis un terminal ouvert **par ssh** vers le Mac (sans session graphique dans le processus) : lancer la même copie et
  **noter** ce qui arrive (`failed` avec quel détail, ou `copied`). Un `copied` n'est acceptable que si le collage dans une
  application de la session graphique rend bien le contenu.
- [ ] Lien contenant des accents ou des caractères non ASCII (nom de fichier `été 日本.png`) : le collage est identique.

## Résultats

À remplir : date, macOS, architecture, terminal utilisé, entrée listée par les Réglages (Kropa ou terminal), cases non
cochées et pourquoi.

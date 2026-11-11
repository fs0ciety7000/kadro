# Kadro — synthèse des équipes (vague 1)

État : **propositions, rien n'est validé.** Documents sources : `product.md`, `architecture.md`, `platform-linux.md`,
`platform-macos.md`, `uploads.md`, `brand/brand.md` (+ `brand/preview.html`).

## Propositions convergentes

| Sujet | Proposition | Équipe |
|---|---|---|
| Licence | GPL-3.0-or-later (compatible FFmpeg LGPL/GPL, x264, dépendances Apache-2.0) | Architecture, confirmé par Envoi |
| Interface | Native par plateforme : GTK4 + libadwaita (gtk4-rs) sous Linux, AppKit (objc2) sous macOS ; cœur et canevas communs | Architecture |
| Processus | Démon résident + CLI `kadro` sans GUI, JSON-RPC sur socket Unix | Architecture, Linux, Envoi |
| Capture Linux | Portail Screenshot puis sélection dans notre overlay (image figée) ; ScreenCast + PipeWire pour vidéo et défilement ; détection par capacités | Linux, Architecture |
| Capture macOS | ScreenCaptureKit via objc2 / screencapturekit | macOS, Architecture |
| Rendu | tiny-skia + cosmic-text (CPU), fichiers `.kadro` rééditables locaux | Architecture |
| Vidéo | GStreamer (Linux), AVFoundation (macOS), GIF en Rust (gif + imagequant) ; pas d'encodeur H.264 livré | Architecture |
| Données | TOML (config), SQLite (historique, file d'envoi), Fluent (FR/EN) | Architecture, Envoi |
| Envoi | reqwest, object_store (S3), russh-sftp, destinations HTTP en TOML, import `.sxcu` converti | Envoi |
| Livraison | CLI et cœur d'abord en tranche verticale capture → fichier → envoi → lien (v0.1), GUI ensuite ; vidéo en v1.1 | Produit |

## Désaccords à trancher (non tranchés)

1. **macOS minimal** — Architecture : 14 (captures SCK). macOS : **15** (micro et clics intégrés au flux, un seul
   pipeline vidéo/audio).
2. **Fichier de secrets chiffré** — Architecture : `age` (scrypt, éprouvé, pas de crypto assemblée à la main).
   Envoi : Argon2id + XChaCha20-Poly1305 maison (paramètres plus forts, mais assemblage à auditer).
3. **Imgur** — le brief l'exige en intégré. Envoi : plus aucun nouveau `client_id` possible (page d'enregistrement
   fermée), bloqué au Royaume-Uni → **le retirer des intégrées** et fournir un modèle de destination personnalisée.
   Produit le classe déjà « Should ».
4. **Vidéo Linux** — Architecture : GStreamer. Linux : compare FFmpeg (extension de codecs Flatpak) ; point signalé
   par l'Architecture elle-même (question 8).
5. **Périmètre v1 vs brief** — Produit repousse après v1 : vidéo/GIF, OCR, habillage, fenêtre propre ; et en
   « Should » SFTP, Imgur, miniature flottante, historique, annotations rééditables. Capture défilante, épinglage et
   affichage des clics exclus tant que les plateformes ne les garantissent pas.
6. **Identité** — Direction artistique recommande A « Repère » ; B « Boucle » plus mémorable. Choix à vous.

## Limites plateformes (dites franchement)

- **GNOME Wayland** : miniature dans un coin et épinglage au-dessus **impossibles** (pas de protocole de position ni
  « always on top ») → notification avec actions, « Toujours au premier plan » manuel (Alt+Espace). Fenêtre propre :
  choix par dialogue du portail. Presse-papiers : copie différée exige le focus → notification « Copier ».
- **Wayland en général** : affichage des clics et touches **impossible** sans assistant evdev (natif seulement,
  désactivé par défaut) → curseur surligné seulement. Capture défilante : défilement manuel + assemblage.
- **wlroots** : pas de portail de raccourcis → raccourci du bureau qui lance `kadro capture area`.
- **macOS sans signature** : « Ouvrir quand même » + mot de passe admin à chaque version téléchargée ; permissions
  (écran, micro, Keychain) **redemandées à chaque mise à jour** ; cask Homebrew officiel impossible (désactivé depuis
  le 01/09/2026). Repli : certificat auto-signé stable du projet (non vérifié par Apple, à tester sur machine réelle),
  `.dmg` sur GitHub Releases, tap personnel qui compile localement. Rappel mensuel de permission inévitable.
- **macOS** : raccourcis Option seul / Option+Maj refusés depuis macOS 15 ; capture défilante sans API dédiée.

## Autres points

- **Homonymie** : un éditeur vidéo libre Windows nommé « Kadro » existe (`kadro-studio/kadro`), paquet npm `kadro`
  pris ; crate, PyPI et Flathub libres. Recommandation : réserver le crate et l'identifiant d'application.
- **Distribution Linux** : Flatpak (GNOME) + paquet natif (KDE et wlroots complets) ; question ouverte : filtrage de
  layer-shell et screencopy pour les applications Flatpak.
- **Risque n° 1** : deux interfaces natives pour un développeur seul. Repli : GTK4 aussi sous macOS, ou Slint.

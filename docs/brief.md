# Kadro — brief

Outil de capture d'écran et d'enregistrement pour **Linux et macOS**. Commande : `kadro`. Langage imposé : **Rust**.

## Produit

Kadro capture, annote et partage. Chaque capture est enregistrée en local (dossier par défaut modifiable). Particularité :
envoi optionnel de chaque capture vers la destination choisie par l'utilisateur (Zipline auto-hébergé, S3 et compatibles,
SFTP, Imgur, ou n'importe quelle API HTTP décrite par configuration, import ShareX `.sxcu`), puis copie automatique du
lien public (URL, Markdown ou HTML) dans le presse-papiers.

- **Cible** : développeurs, admins système, rédacteurs de documentation, équipes support, souvent adeptes de
  l'auto-hébergement.
- **Références d'expérience** : CleanShot X, ShareX, macshot.
- **Principes** : natif et léger ; local d'abord (aucune requête réseau non configurée, aucune télémétrie) ; tout au
  clavier ; tout scriptable (CLI couvrant toutes les actions de l'interface).

## Contraintes

- Rust. Linux et macOS à qualité égale autant que possible.
- Aucun service d'envoi imposé ni requis.
- Secrets jamais stockés en clair : trousseau système (Keychain, Secret Service) ; **repli : fichier chiffré par mot
  de passe maître** quand aucun trousseau n'est disponible.
- Interface en français et en anglais.
- **Projet libre** (licence à choisir, compatible avec les dépendances retenues, notamment FFmpeg).
- **Pas de signature Apple** (pas de compte Developer) : signature ad hoc ; conséquences à documenter honnêtement.
- **Pas d'AUR** (pas de compte Arch).
- **Développeur seul, pas de délai** : le périmètre de la v1 doit être tenable par une personne.
- Linux : priorité **GNOME et KDE sous Wayland** ; wlroots (Sway, Hyprland) et X11 au mieux.

## Fonctionnalités attendues

**Capture** : zone, plein écran, écran actif, avec loupe au pixel et dimensions ; fenêtre « propre » (isolée, ombre,
fond uni/dégradé/transparent) ; capture différée (compte à rebours) ; capture défilante ; reprise de la dernière zone,
pipette, règle en pixels ; OCR d'une zone, détection de QR codes.

**Annotation** : flèches, cadres, ellipses, lignes, texte, crayon ; flou, pixellisation, masque plein ; détection
assistée des secrets ; compteurs d'étapes ; surligneur, projecteur ; habillage (fond, marge, ombre, préréglages) ;
annotations rééditables après coup.

**Vidéo et GIF** : zone/fenêtre/écran en MP4 et GIF, audio système et micro en option, pause/reprise ; affichage des
clics et des raccourcis ; coupe début/fin avant export.

**Après la capture** : miniature flottante glisser-déposable ; épinglage au-dessus des fenêtres ; enregistrement local ;
historique avec recherche ; chaîne d'actions configurable ; suppression des métadonnées avant envoi.

**Envoi** : Zipline, S3, SFTP, Imgur ; destination HTTP personnalisée + import `.sxcu` ; plusieurs profils, un par
défaut ; copie du lien (URL/Markdown/HTML) ; file d'attente avec reprise ; envoi par morceaux.

**Transversal** : raccourcis globaux configurables ; CLI complète ; modèles de nom de fichier.

## Méthode

- Aucun code avant validation des propositions des équipes.
- Chaque équipe compare au moins deux options et justifie son choix dans un document court de `docs/`.
- Fonctionnalité impossible ou dégradée sur une plateforme : le dire clairement, proposer un repli. Jamais de succès simulé.
- Désaccords entre équipes : signalés, pas tranchés en silence.

# CLAUDE.md

> Lu automatiquement à chaque session. Court exprès. Les règles générales (langue, secrets, routage des skills et des agents) sont dans `~/.claude/CLAUDE.md`.

@AGENTS.md

## Projet courant

- **Nom** : Kropa (commande `kropa`), ex-Kadro. Capture d'écran, annotation et envoi vers la destination de
  l'utilisateur, pour Linux (GNOME/KDE Wayland d'abord) et macOS 15+. Licence GPL-3.0-or-later.
- **Type** : application de bureau Rust (+ `brand`, `design-ui`).
- **Stack** : voir `docs/memory/stack.md` et `docs/architecture.md`.
- **Commandes** (à partir du lot 0) : `cargo build --workspace --locked`, `cargo nextest run --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo deny check`.
- **Déploiement** : aucun (paquets de bureau, voir `DEPLOY.md`).

## État d'avancement

- Conception validée le 11/11/2026. Faits : lots 0, D1, S, D2 (sauf D2.4) ; lot 1 tâches 1.1-1.5, 1.7-1.11 (+ suites de revue). Suite : D2.4 puis essais réels 1.12 (utilisateur), puis lot 2. Suivi : `docs/plan.md`, relais : `docs/memory/handoff.md`.

## Documents du projet

- `docs/brief.md` : objectif, périmètre, contraintes. `docs/decisions.md` : décisions validées (prioritaires).
- `docs/plan.md` : lots, tâches, agents, critères de fin.
- `docs/product.md`, `architecture.md`, `platform-linux.md`, `platform-macos.md`, `uploads.md`,
  `quality-release.md`, `brand/` : propositions des équipes (peuvent contenir des éléments dépassés par `decisions.md`).
- `DESIGN.md` : direction visuelle, écrite au lot D2. À respecter pour toute interface.

## Équipe

| Agent | Rôle ici | Modèle / effort | Source |
|---|---|---|---|
| `architect` | conception des lots, spec CLI, modèle de secrets | opus / high | kit |
| `backend-dev` | cœur Rust : config, CLI, rendu, document, envoi, store, IPC | sonnet / medium | kit |
| `linux-desktop-dev` | GTK4/libadwaita, portails, Wayland, Secret Service | sonnet / high | projet |
| `macos-desktop-dev` | objc2/AppKit, ScreenCaptureKit, TCC, Keychain, `.app` | sonnet / high | projet |
| `qa-tester` | tests, instantanés d'image, check-lists manuelles | sonnet / medium | kit |
| `code-reviewer` | relecture de chaque PR | sonnet / high | kit |
| `security-reviewer` | secrets, réseau, dépendances, signature, release | opus / xhigh | kit |

Appui : `brand-identity`, `ui-designer`, `design-critic`, `copywriter`, `devops` (kit).

## Spécifique à ce projet

- Jamais de succès simulé : une fonction impossible ou dégradée sur une plateforme renvoie un statut explicite et son
  repli documenté (`docs/platform-*.md`).
- Local d'abord : aucune requête réseau non configurée, aucune télémétrie, aucune vérification de mise à jour
  automatique.
- Secrets : trousseau système, repli fichier `age` ; jamais en clair, ni en argument de commande, ni en variable
  d'environnement. Aucune variable d'environnement de projet (`.env.example` vide).
- Désaccords entre équipes : signalés à l'utilisateur, jamais tranchés en silence.

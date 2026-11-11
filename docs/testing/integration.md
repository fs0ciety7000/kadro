# Tests d'intégration : vrais serveurs et « zéro requête » (lot 1.11)

> Deux garanties que les doubles (`wiremock`, portail factice) ne donnent pas : `kropa upload` fonctionne contre un
> vrai Zipline 4, et `kropa` sans destination configurée n'ouvre **aucune** connexion réseau.

## 1. Zipline réel (`tests/integration/`)

| Fichier | Rôle |
|---|---|
| `compose.yaml` | Zipline 4.8.3 + PostgreSQL 18.6 derrière Caddy 2.11.9 (profil par défaut) ; Garage 2.5.0 (profil `s3`, lot 2.7) et OpenSSH 10.3 (profil `sftp`, lot 4.9), **déclarés mais pas encore exercés**. Images épinglées par **digest** de l'index multi-architecture, vérifié auprès du registre le 11/11/2026 ; Renovate met à jour tag et digest. |
| `Caddyfile`, `garage.toml` | Configuration sans secret. |
| `run.sh` | `test` (monte, teste, démonte même en cas d'échec), `up`, `down`. |
| `crates/kropa-cli/tests/zipline_integration.rs` | Le test (`#[ignore]` : `cargo nextest run` ne le lance pas). |

```sh
tests/integration/run.sh test                  # Docker, openssl, curl, jq, cargo-nextest, gnome-keyring, dbus-run-session
tests/integration/run.sh up --profile s3       # à la main : serveurs gardés, `down` pour tout retirer
tests/integration/run.sh down
```

**Ce que fait le test.** Un `kropa upload` réel vers `https://localhost:18443` (préréglage Zipline de
`destinations.toml`), le jeton venant d'un vrai GNOME Keyring sur un bus D-Bus privé. Il vérifie : code 0, **stdout =
le lien seul** (`https://localhost:18443/u/<nom>.png`), aucun jeton dans stdout ni stderr, puis un `GET` du lien qui
rend les octets envoyés. Un essai témoin refait le même envoi avec un magasin de confiance qui ne connaît pas la CA :
il doit échouer (le certificat est bien vérifié, aucune exception).

**Secrets.** Rien n'est commité : pas de jeton, pas de fichier d'environnement. `run.sh` génère à chaque exécution les
secrets de PostgreSQL, Zipline, Garage et SSH, la CA et le certificat TLS dans un dossier privé (`$XDG_RUNTIME_DIR` ou
`/tmp`, mode 0700), que `down` supprime. Compose les reçoit par son propre environnement (`${…:?}` : il refuse de
démarrer sans eux, d'où `run.sh` plutôt que `docker compose` seul). Le premier utilisateur de Zipline est créé par son
API ; son jeton est écrit dans un fichier 0600 et le test le passe à `kropa secrets set zipline token --secret-stdin`.
Jamais en argument, jamais dans l'environnement de `kropa`. Sur GitHub Actions, les valeurs sont masquées dans le journal.

**Réseau.** Seul Caddy publie un port, sur `127.0.0.1:18443` (variable `KROPA_IT_HTTPS_PORT`). Garage (`13900`,
`13903`) et OpenSSH (`12222`) aussi sur `127.0.0.1`. PostgreSQL et Zipline ne sont pas publiés ; leurs données vivent en
mémoire (`tmpfs`).

**TLS.** Kropa refuse un secret en HTTP et n'a aucune option pour faire confiance à un certificat. Zipline est donc
derrière Caddy avec un certificat d'une CA jetable, limitée à `localhost` par une contrainte de noms, dont la clé est
détruite dès le certificat signé. Cette CA n'arrive à Kropa que par le **magasin de confiance du système**
(`rustls-platform-verifier`) :

- `--trust system` (défaut en CI et en root) : la CA est installée par `update-ca-certificates` (famille Debian ;
  `sudo` si besoin), retirée par `down`. Pour un runner ou un conteneur.
- `--trust bundle` (défaut sur un poste de développement) : rien n'est installé ; la CA est ajoutée à **une copie** du
  bundle système que le test passe au processus par `SSL_CERT_FILE`, la variable standard du chargeur de certificats
  natifs (convention OpenSSL), pas une option de Kropa.

**CI.** Job `integration` de `.github/workflows/ci.yml` : sur `push` vers `main` et `workflow_dispatch` seulement
(quota Actions), pas sur chaque PR. Il lance `run.sh test --trust system`.

**Limites.** Linux seulement (les runners macOS n'ont pas Docker). Ni S3 ni SFTP ne sont testés ici : Garage et OpenSSH
démarrent (vérifié), la couche et le test viennent aux lots 2.7 et 4.9. L'envoi par morceaux de Zipline n'est pas
encore dans le code de `kropa-upload`, donc pas dans ce test.

## 2. « Zéro requête » (`crates/kropa-cli/tests/no_network.rs`)

Test ordinaire (lancé par `cargo nextest run`, donc par le job `test` Linux). Pour `--help`, `--version`, `version`,
`doctor`, `dest list`, `secrets status` (avec et sans `--json`) et `upload` sans destination (code 6), deux preuves :

1. le binaire s'exécute dans un espace de noms réseau vide (`unshare -rn`) ;
2. `strace -f -e trace=%network` journalise les appels de socket, et le test échoue au moindre `socket`, `bind`,
   `connect` ou `send*` d'une famille réseau (`AF_INET`, `AF_INET6`, `AF_PACKET`…). `AF_UNIX` (bus D-Bus) et
   `AF_NETLINK` sont permis. Chaque commande tourne sans bus, puis sur un bus de session privé.

Le détecteur n'est pas vide : un essai témoin (`bash` ouvrant une connexion TCP) doit être vu, et un test pur de
l'analyseur couvre les familles locales et réseau.

**Variable `KROPA_REQUIRE_NO_NETWORK_TESTS`** (posée par la CI Linux) : sans elle, l'absence de `strace` fait passer le
test avec un message `SKIPPED`, et un `unshare -rn` refusé le fait tourner avec `strace` seul (message
`NETWORK NAMESPACE UNAVAILABLE`). Avec elle, les deux sont des échecs. Ubuntu 24.04 refuse les espaces de noms
utilisateur non privilégiés par AppArmor : le job `test` exécute d'abord
`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`.

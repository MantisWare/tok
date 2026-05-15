# TOK - Documentation fonctionnelle complete

> **tok (Token Optimization Kit)** -- Proxy CLI haute performance qui reduit la consommation de tokens LLM de 60 a 90%.

Binaire Rust unique, zero dependances externes, overhead < 10ms par commande.

---

## Table des matieres

1. [Vue d'ensemble](#vue-densemble)
2. [Drapeaux globaux](#drapeaux-globaux)
3. [Commandes Fichiers](#commandes-fichiers)
4. [Commandes Git](#commandes-git)
5. [Commandes GitHub CLI](#commandes-github-cli)
6. [Commandes Test](#commandes-test)
7. [Commandes Build et Lint](#commandes-build-et-lint)
8. [Commandes Formatage](#commandes-formatage)
9. [Gestionnaires de paquets](#gestionnaires-de-paquets)
10. [Conteneurs et orchestration](#conteneurs-et-orchestration)
11. [Donnees et reseau](#donnees-et-reseau)
12. [Cloud et bases de donnees](#cloud-et-bases-de-donnees)
13. [Stacked PRs (Graphite)](#stacked-prs-graphite)
14. [Analytique et suivi](#analytique-et-suivi)
15. [Systeme de hooks](#systeme-de-hooks)
16. [Configuration](#configuration)
17. [Systeme Tee (recuperation de sortie)](#systeme-tee)
18. [Telemetrie](#telemetrie)

---

## Vue d'ensemble

tok agit comme un proxy entre un LLM (Claude Code, Gemini CLI, etc.) et les commandes systeme. Quatre strategies de filtrage sont appliquees selon le type de commande :

| Strategie | Description | Exemple |
|-----------|-------------|---------|
| **Filtrage intelligent** | Supprime le bruit (commentaires, espaces, boilerplate) | `ls -la` -> arbre compact |
| **Regroupement** | Agregation par repertoire, par type d'erreur, par regle | Tests groupes par fichier |
| **Troncature** | Conserve le contexte pertinent, supprime la redondance | Diff condense |
| **Deduplication** | Fusionne les lignes de log repetees avec compteurs | `error x42` |

### Mecanisme de fallback

Si tok ne reconnait pas une sous-commande, il execute la commande brute (passthrough) et enregistre l'evenement dans la base de suivi. Cela garantit que tok est **toujours sur** a utiliser -- aucune commande ne sera bloquee.

---

## Drapeaux globaux

Ces drapeaux s'appliquent a **toutes** les sous-commandes :

| Drapeau | Court | Description |
|---------|-------|-------------|
| `--verbose` | `-v` | Augmenter la verbosite (-v, -vv, -vvv). Montre les details de filtrage. |
| `--ultra-compact` | `-u` | Mode ultra-compact : icones ASCII, format inline. Economies supplementaires. |
| `--skip-env` | -- | Definit `SKIP_ENV_VALIDATION=1` pour les processus enfants (Next.js, tsc, lint, prisma). |

**Exemples :**

```bash
tok -v git status          # Status compact + details de filtrage sur stderr
tok -vvv cargo test        # Verbosite maximale (debug)
tok -u git log             # Log ultra-compact, icones ASCII
tok --skip-env next build  # Desactive la validation d'env de Next.js
```

---

## Commandes Fichiers

### `tok ls` -- Listage de repertoire

**Objectif :** Remplace `ls` et `tree` avec une sortie optimisee en tokens.

**Syntaxe :**
```bash
tok ls [args...]
```

Tous les drapeaux natifs de `ls` sont supportes (`-l`, `-a`, `-h`, `-R`, etc.).

**Economies :** ~80% de reduction de tokens

**Avant / Apres :**
```
# ls -la (45 lignes, ~800 tokens)          # tok ls (12 lignes, ~150 tokens)
drwxr-xr-x  15 user staff 480 ...          my-project/
-rw-r--r--   1 user staff 1234 ...          +-- src/ (8 files)
-rw-r--r--   1 user staff 567 ...           |   +-- main.rs
...40 lignes de plus...                     +-- Cargo.toml
                                            +-- README.md
```

---

### `tok tree` -- Arbre de repertoire

**Objectif :** Proxy vers `tree` natif avec sortie filtree.

**Syntaxe :**
```bash
tok tree [args...]
```

Supporte tous les drapeaux natifs de `tree` (`-L`, `-d`, `-a`, etc.).

**Economies :** ~80%

---

### `tok read` -- Lecture de fichier

**Objectif :** Remplace `cat`, `head`, `tail` avec un filtrage intelligent du contenu.

**Syntaxe :**
```bash
tok read <fichier> [options]
tok read - [options]          # Lecture depuis stdin
```

**Options :**

| Option | Court | Defaut | Description |
|--------|-------|--------|-------------|
| `--level` | `-l` | `minimal` | Niveau de filtrage : `none`, `minimal`, `aggressive` |
| `--max-lines` | `-m` | illimite | Nombre maximum de lignes |
| `--line-numbers` | `-n` | non | Afficher les numeros de ligne |

**Niveaux de filtrage :**

| Niveau | Description | Economies |
|--------|-------------|-----------|
| `none` | Aucun filtrage, sortie brute | 0% |
| `minimal` | Supprime commentaires et lignes vides excessives | ~30% |
| `aggressive` | Signatures uniquement (supprime les corps de fonctions) | ~74% |

**Avant / Apres (mode aggressive) :**
```
# cat main.rs (~200 lignes)                # tok read main.rs -l aggressive (~50 lignes)
fn main() -> Result<()> {                   fn main() -> Result<()> { ... }
    let config = Config::load()?;           fn process_data(input: &str) -> Vec<u8> { ... }
    let data = process_data(&input);        struct Config { ... }
    for item in data {                      impl Config { fn load() -> Result<Self> { ... } }
        println!("{}", item);
    }
    Ok(())
}
...
```

**Langages supportes pour le filtrage :** Rust, Python, JavaScript, TypeScript, Go, C, C++, Java, Ruby, Shell.

---

### `tok smart` -- Resume heuristique

**Objectif :** Genere un resume technique de 2 lignes pour un fichier source.

**Syntaxe :**
```bash
tok smart <fichier> [--model heuristic] [--force-download]
```

**Economies :** ~95%

**Exemple :**
```
$ tok smart src/tracking.rs
SQLite-based token tracking system for command executions.
Records input/output tokens, savings %, execution times with 90-day retention.
```

---

### `tok find` -- Recherche de fichiers

**Objectif :** Remplace `find` et `fd` avec une sortie compacte groupee par repertoire.

**Syntaxe :**
```bash
tok find [args...]
```

Supporte a la fois la syntaxe TOK et la syntaxe native `find` (`-name`, `-type`, etc.).

**Economies :** ~80%

**Avant / Apres :**
```
# find . -name "*.rs" (30 lignes)           # tok find "*.rs" . (8 lignes)
./src/main.rs                                src/ (12 .rs)
./src/git.rs                                   main.rs, git.rs, config.rs
./src/config.rs                                tracking.rs, filter.rs, utils.rs
./src/tracking.rs                              ...6 more
./src/filter.rs                              tests/ (3 .rs)
./src/utils.rs                                 test_git.rs, test_ls.rs, test_filter.rs
...24 lignes de plus...
```

---

### `tok grep` -- Recherche dans le contenu

**Objectif :** Remplace `grep` et `rg` avec une sortie groupee par fichier, tronquee.

**Syntaxe :**
```bash
tok grep <pattern> [chemin] [options]
```

**Options :**

| Option | Court | Defaut | Description |
|--------|-------|--------|-------------|
| `--max-len` | `-l` | 80 | Longueur maximale de ligne |
| `--max` | `-m` | 50 | Nombre maximum de resultats |
| `--context-only` | `-c` | non | Afficher uniquement le contexte du match |
| `--file-type` | `-t` | tous | Filtrer par type (ts, py, rust, etc.) |
| `--line-numbers` | `-n` | oui | Numeros de ligne (toujours actif) |

Les arguments supplementaires sont transmis a `rg` (ripgrep).

**Economies :** ~80%

**Avant / Apres :**
```
# rg "fn run" (20 lignes)                   # tok grep "fn run" (10 lignes)
src/git.rs:45:pub fn run(...)                src/git.rs
src/git.rs:120:fn run_status(...)              45: pub fn run(...)
src/ls.rs:12:pub fn run(...)                   120: fn run_status(...)
src/ls.rs:25:fn run_tree(...)                src/ls.rs
...                                            12: pub fn run(...)
                                               25: fn run_tree(...)
```

---

### `tok diff` -- Diff condense

**Objectif :** Diff ultra-condense entre deux fichiers (uniquement les lignes modifiees).

**Syntaxe :**
```bash
tok diff <fichier1> <fichier2>
tok diff <fichier1>              # Stdin comme second fichier
```

**Economies :** ~60%

---

### `tok wc` -- Comptage compact

**Objectif :** Remplace `wc` avec une sortie compacte (supprime les chemins et le padding).

**Syntaxe :**
```bash
tok wc [args...]
```

Supporte tous les drapeaux natifs de `wc` (`-l`, `-w`, `-c`, etc.).

---

## Commandes Git

### Vue d'ensemble

Toutes les sous-commandes git sont supportees. Les commandes non reconnues sont transmises directement a git (passthrough).

**Options globales git :**

| Option | Description |
|--------|-------------|
| `-C <path>` | Changer de repertoire avant execution |
| `-c <key=value>` | Surcharger une config git |
| `--git-dir <path>` | Chemin vers le repertoire .git |
| `--work-tree <path>` | Chemin vers le working tree |
| `--no-pager` | Desactiver le pager |
| `--no-optional-locks` | Ignorer les locks optionnels |
| `--bare` | Traiter comme repo bare |
| `--literal-pathspecs` | Pathspecs literals |

---

### `tok git status` -- Status compact

**Economies :** ~80%

```bash
tok git status [args...]    # Supporte tous les drapeaux git status
```

**Avant / Apres :**
```
# git status (~20 lignes, ~400 tokens)      # tok git status (~5 lignes, ~80 tokens)
On branch main                               main | 3M 1? 1A
Your branch is up to date with               M src/main.rs
  'origin/main'.                              M src/git.rs
                                              M tests/test_git.rs
Changes not staged for commit:                ? new_file.txt
  (use "git add <file>..." to update)        A staged_file.rs
  modified:   src/main.rs
  modified:   src/git.rs
  ...
```

---

### `tok git log` -- Historique compact

**Economies :** ~80%

```bash
tok git log [args...]    # Supporte --oneline, --graph, --all, -n, etc.
```

**Avant / Apres :**
```
# git log (50+ lignes)                      # tok git log -n 5 (5 lignes)
commit abc123def... (HEAD -> main)           abc123 Fix token counting bug
Author: User <user@email.com>               def456 Add vitest support
Date:   Mon Jan 15 10:30:00 2024            789abc Refactor filter engine
                                             012def Update README
    Fix token counting bug                   345ghi Initial commit
...
```

---

### `tok git diff` -- Diff compact

**Economies :** ~75%

```bash
tok git diff [args...]    # Supporte --stat, --cached, --staged, etc.
```

**Avant / Apres :**
```
# git diff (~100 lignes)                    # tok git diff (~25 lignes)
diff --git a/src/main.rs b/src/main.rs      src/main.rs (+5/-2)
index abc123..def456 100644                    +  let config = Config::load()?;
--- a/src/main.rs                              +  config.validate()?;
+++ b/src/main.rs                              -  // old code
@@ -10,6 +10,8 @@                              -  let x = 42;
   fn main() {                               src/git.rs (+1/-1)
+    let config = Config::load()?;              ~  format!("ok {}", branch)
...30 lignes de headers et contexte...
```

---

### `tok git show` -- Show compact

**Economies :** ~80%

```bash
tok git show [args...]
```

Affiche le resume du commit + stat + diff compact.

---

### `tok git add` -- Add ultra-compact

**Economies :** ~92%

```bash
tok git add [args...]    # Supporte -A, -p, --all, etc.
```

**Sortie :** `ok` (un seul mot)

---

### `tok git commit` -- Commit ultra-compact

**Economies :** ~92%

```bash
tok git commit -m "message" [args...]    # Supporte -a, --amend, --allow-empty, etc.
```

**Sortie :** `ok abc1234` (confirmation + hash court)

---

### `tok git push` -- Push ultra-compact

**Economies :** ~92%

```bash
tok git push [args...]    # Supporte -u, remote, branch, etc.
```

**Avant / Apres :**
```
# git push (15 lignes, ~200 tokens)         # tok git push (1 ligne, ~10 tokens)
Enumerating objects: 5, done.                ok main
Counting objects: 100% (5/5), done.
Delta compression using up to 8 threads
...
```

---

### `tok git pull` -- Pull ultra-compact

**Economies :** ~92%

```bash
tok git pull [args...]
```

**Sortie :** `ok 3 files +10 -2`

---

### `tok git branch` -- Branches compact

```bash
tok git branch [args...]    # Supporte -d, -D, -m, etc.
```

Affiche branche courante, branches locales, branches distantes de facon compacte.

---

### `tok git fetch` -- Fetch compact

```bash
tok git fetch [args...]
```

**Sortie :** `ok fetched (N new refs)`

---

### `tok git stash` -- Stash compact

```bash
tok git stash [list|show|pop|apply|drop|push] [args...]
```

---

### `tok git worktree` -- Worktree compact

```bash
tok git worktree [add|remove|prune|list] [args...]
```

---

### Passthrough git

Toute sous-commande git non listee ci-dessus est executee directement :

```bash
tok git rebase main        # Execute git rebase main
tok git cherry-pick abc    # Execute git cherry-pick abc
tok git tag v1.0.0         # Execute git tag v1.0.0
```

---

## Commandes GitHub CLI

### `tok gh` -- GitHub CLI compact

**Objectif :** Remplace `gh` avec une sortie optimisee.

**Syntaxe :**
```bash
tok gh <sous-commande> [args...]
```

**Sous-commandes supportees :**

| Commande | Description | Economies |
|----------|-------------|-----------|
| `tok gh pr list` | Liste des PRs compacte | ~80% |
| `tok gh pr view <num>` | Details d'une PR + checks | ~87% |
| `tok gh pr checks` | Status des checks CI | ~79% |
| `tok gh issue list` | Liste des issues compacte | ~80% |
| `tok gh run list` | Status des workflow runs | ~82% |
| `tok gh api <endpoint>` | Reponse API compacte | ~26% |

**Avant / Apres :**
```
# gh pr list (~30 lignes)                   # tok gh pr list (~10 lignes)
Showing 10 of 15 pull requests in org/repo   #42 feat: add vitest (open, 2d)
                                              #41 fix: git diff crash (open, 3d)
#42  feat: add vitest support                 #40 chore: update deps (merged, 5d)
  user opened about 2 days ago                #39 docs: add guide (merged, 1w)
  ... labels: enhancement
...
```

---

## Commandes Test

### `tok test` -- Wrapper de tests generique

**Objectif :** Execute n'importe quelle commande de test et affiche uniquement les echecs.

**Syntaxe :**
```bash
tok test <commande...>
```

**Economies :** ~90%

**Exemple :**
```bash
tok test cargo test
tok test npm test
tok test bun test
tok test pytest
```

**Avant / Apres :**
```
# cargo test (200+ lignes en cas d'echec)   # tok test cargo test (~20 lignes)
running 15 tests                             FAILED: 2/15 tests
test utils::test_parse ... ok                  test_edge_case: assertion failed
test utils::test_format ... ok                 test_overflow: panic at utils.rs:18
test utils::test_edge_case ... FAILED
...150 lignes de backtrace...
```

---

### `tok err` -- Erreurs/avertissements uniquement

**Objectif :** Execute une commande et ne montre que les erreurs et avertissements.

**Syntaxe :**
```bash
tok err <commande...>
```

**Economies :** ~80%

**Exemple :**
```bash
tok err npm run build
tok err cargo build
```

---

### `tok cargo test` -- Tests Rust

**Economies :** ~90%

```bash
tok cargo test [args...]
```

N'affiche que les echecs. Supporte tous les arguments de `cargo test`.

---

### `tok cargo nextest` -- Tests Rust (nextest)

```bash
tok cargo nextest [run|list|--lib] [args...]
```

Filtre la sortie de `cargo nextest` pour n'afficher que les echecs.

---

### `tok vitest run` -- Tests Vitest

**Economies :** ~99.5%

```bash
tok vitest run [args...]
```

---

### `tok playwright test` -- Tests E2E Playwright

**Economies :** ~94%

```bash
tok playwright [args...]
```

---

### `tok pytest` -- Tests Python

**Economies :** ~90%

```bash
tok pytest [args...]
```

---

### `tok go test` -- Tests Go

**Economies :** ~90%

```bash
tok go test [args...]
```

Utilise le streaming JSON NDJSON de Go pour un filtrage precis.

---

## Commandes Build et Lint

### `tok cargo build` -- Build Rust

**Economies :** ~80%

```bash
tok cargo build [args...]
```

Supprime les lignes "Compiling...", ne conserve que les erreurs et le resultat final.

---

### `tok cargo check` -- Check Rust

**Economies :** ~80%

```bash
tok cargo check [args...]
```

Supprime les lignes "Checking...", ne conserve que les erreurs.

---

### `tok cargo clippy` -- Clippy Rust

**Economies :** ~80%

```bash
tok cargo clippy [args...]
```

Regroupe les avertissements par regle de lint.

---

### `tok cargo install` -- Install Rust

```bash
tok cargo install [args...]
```

Supprime la compilation des dependances, ne conserve que le resultat d'installation et les erreurs.

---

### `tok tsc` -- TypeScript Compiler

**Economies :** ~83%

```bash
tok tsc [args...]
```

Regroupe les erreurs TypeScript par fichier et par code d'erreur.

**Avant / Apres :**
```
# tsc --noEmit (50 lignes)                  # tok tsc (15 lignes)
src/api.ts(12,5): error TS2345: ...          src/api.ts (3 errors)
src/api.ts(15,10): error TS2345: ...           TS2345: Argument type mismatch (x2)
src/api.ts(20,3): error TS7006: ...            TS7006: Parameter implicitly has 'any'
src/utils.ts(5,1): error TS2304: ...         src/utils.ts (1 error)
...                                            TS2304: Cannot find name 'foo'
```

---

### `tok lint` -- ESLint / Biome

**Economies :** ~84%

```bash
tok lint [args...]
tok lint biome [args...]
```

Regroupe les violations par regle et par fichier. Auto-detecte le linter.

---

### `tok prettier` -- Verification du formatage

**Economies :** ~70%

```bash
tok prettier [args...]    # ex: tok prettier --check .
```

Affiche uniquement les fichiers necessitant un formatage.

---

### `tok format` -- Formateur universel

```bash
tok format [args...]
```

Auto-detecte le formateur du projet (prettier, black, ruff format) et applique un filtre compact.

---

### `tok next build` -- Build Next.js

**Economies :** ~87%

```bash
tok next [args...]
```

Sortie compacte avec metriques de routes.

---

### `tok ruff` -- Linter/formateur Python

**Economies :** ~80%

```bash
tok ruff check [args...]
tok ruff format --check [args...]
```

Sortie JSON compressee.

---

### `tok mypy` -- Type checker Python

```bash
tok mypy [args...]
```

Regroupe les erreurs de type par fichier.

---

### `tok golangci-lint` -- Linter Go

**Economies :** ~85%

```bash
tok golangci-lint run [args...]
```

Sortie JSON compressee.

---

## Commandes Formatage

### `tok prettier` -- Prettier

```bash
tok prettier --check .
tok prettier --write src/
```

---

### `tok format` -- Detecteur universel

```bash
tok format [args...]
```

Detecte automatiquement : prettier, black, ruff format, rustfmt. Applique un filtre compact unifie.

---

## Gestionnaires de paquets

### `tok pnpm` -- pnpm

| Commande | Description | Economies |
|----------|-------------|-----------|
| `tok pnpm list [-d N]` | Arbre de dependances compact | ~70% |
| `tok pnpm outdated` | Paquets obsoletes : `pkg: old -> new` | ~80% |
| `tok pnpm install [pkgs...]` | Filtre les barres de progression | ~60% |
| `tok pnpm build` | Delegue au filtre Next.js | ~87% |
| `tok pnpm typecheck` | Delegue au filtre tsc | ~83% |

Les sous-commandes non reconnues sont transmises directement a pnpm (passthrough).

---

### `tok npm` -- npm

```bash
tok npm [args...]    # ex: tok npm run build
```

Filtre le boilerplate npm (barres de progression, en-tetes, etc.).

---

### `tok npx` -- npx avec routage intelligent

```bash
tok npx [args...]
```

Route intelligemment vers les filtres specialises :
- `tok npx tsc` -> filtre tsc
- `tok npx eslint` -> filtre lint
- `tok npx prisma` -> filtre prisma
- Autres -> passthrough filtre

---

### `tok pip` -- pip / uv

```bash
tok pip list              # Liste des paquets (auto-detecte uv)
tok pip outdated          # Paquets obsoletes
tok pip install <pkg>     # Installation
```

Auto-detecte `uv` si disponible et l'utilise a la place de `pip`.

---

### `tok deps` -- Resume des dependances

**Objectif :** Resume compact des dependances du projet.

```bash
tok deps [chemin]    # Defaut: repertoire courant
```

Auto-detecte : `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `Gemfile`, etc.

**Economies :** ~70%

---

### `tok prisma` -- ORM Prisma

| Commande | Description |
|----------|-------------|
| `tok prisma generate` | Generation du client (supprime l'ASCII art) |
| `tok prisma migrate dev [--name N]` | Creer et appliquer une migration |
| `tok prisma migrate status` | Status des migrations |
| `tok prisma migrate deploy` | Deployer en production |
| `tok prisma db-push` | Push du schema |

---

## Conteneurs et orchestration

### `tok docker` -- Docker

| Commande | Description | Economies |
|----------|-------------|-----------|
| `tok docker ps` | Liste compacte des conteneurs | ~80% |
| `tok docker images` | Liste compacte des images | ~80% |
| `tok docker logs <conteneur>` | Logs dedupliques | ~70% |
| `tok docker compose ps` | Services Compose compacts | ~80% |
| `tok docker compose logs [service]` | Logs Compose dedupliques | ~70% |
| `tok docker compose build [service]` | Resume du build | ~60% |

Les sous-commandes non reconnues sont transmises directement (passthrough).

**Avant / Apres :**
```
# docker ps (lignes longues, ~30 tokens/ligne)    # tok docker ps (~10 tokens/ligne)
CONTAINER ID   IMAGE          COMMAND     ...      web  nginx:1.25 Up 2d (healthy)
abc123def456   nginx:1.25     "/dock..."  ...      db   postgres:16 Up 2d (healthy)
789012345678   postgres:16    "docker..."           redis redis:7 Up 1d
```

---

### `tok kubectl` -- Kubernetes

| Commande | Description | Options |
|----------|-------------|---------|
| `tok kubectl pods [-n ns] [-A]` | Liste compacte des pods | Namespace ou tous |
| `tok kubectl services [-n ns] [-A]` | Liste compacte des services | Namespace ou tous |
| `tok kubectl logs <pod> [-c container]` | Logs dedupliques | Container specifique |

Les sous-commandes non reconnues sont transmises directement (passthrough).

---

## Donnees et reseau

### `tok json` -- Structure JSON

**Objectif :** Affiche la structure d'un fichier JSON sans les valeurs.

```bash
tok json <fichier> [--depth N]    # Defaut: profondeur 5
tok json -                         # Depuis stdin
```

**Economies :** ~60%

**Avant / Apres :**
```
# cat package.json (50 lignes)              # tok json package.json (10 lignes)
{                                            {
  "name": "my-app",                            name: string
  "version": "1.0.0",                         version: string
  "dependencies": {                            dependencies: { 15 keys }
    "react": "^18.2.0",                        devDependencies: { 8 keys }
    "next": "^14.0.0",                         scripts: { 6 keys }
    ...15 dependances...                     }
  },
  ...
}
```

---

### `tok env` -- Variables d'environnement

```bash
tok env                    # Toutes les variables (sensibles masquees)
tok env -f AWS             # Filtrer par nom
tok env --show-all         # Inclure les valeurs sensibles
```

Les variables sensibles (tokens, secrets, mots de passe) sont masquees par defaut : `AWS_SECRET_ACCESS_KEY=***`.

---

### `tok log` -- Logs dedupliques

**Objectif :** Filtre et deduplique la sortie de logs.

```bash
tok log <fichier>     # Depuis un fichier
tok log               # Depuis stdin (pipe)
```

Les lignes repetees sont fusionnees : `[ERROR] Connection refused (x42)`.

**Economies :** ~60-80% (selon la repetitivite)

---

### `tok curl` -- HTTP avec detection JSON

```bash
tok curl [args...]
```

Auto-detecte les reponses JSON et affiche le schema au lieu du contenu complet.

---

### `tok wget` -- Telechargement compact

```bash
tok wget <url> [args...]
tok wget -O - <url>           # Sortie vers stdout
```

Supprime les barres de progression et le bruit.

---

### `tok summary` -- Resume heuristique

**Objectif :** Execute une commande et genere un resume heuristique de la sortie.

```bash
tok summary <commande...>
```

Utile pour les commandes longues dont la sortie n'a pas de filtre dedie.

---

### `tok proxy` -- Passthrough avec suivi

**Objectif :** Execute une commande **sans filtrage** mais enregistre l'utilisation pour le suivi.

```bash
tok proxy <commande...>
```

Utile pour le debug : comparer la sortie brute avec la sortie filtree.

---

## Cloud et bases de donnees

### `tok aws` -- AWS CLI

```bash
tok aws <service> [args...]
```

Force la sortie JSON et compresse le resultat. Supporte tous les services AWS (sts, s3, ec2, ecs, rds, cloudformation, etc.).

---

### `tok psql` -- PostgreSQL

```bash
tok psql [args...]
```

Supprime les bordures de tableaux et compresse la sortie.

---

## Stacked PRs (Graphite)

### `tok gt` -- Graphite

| Commande | Description |
|----------|-------------|
| `tok gt log` | Stack log compact |
| `tok gt submit` | Submit compact |
| `tok gt sync` | Sync compact |
| `tok gt restack` | Restack compact |
| `tok gt create` | Create compact |
| `tok gt branch` | Branch info compact |

Les sous-commandes non reconnues sont transmises directement ou detectees comme passthrough git.

---

## Analytique et suivi

### Systeme de tracking

TOK enregistre chaque execution de commande dans une base SQLite :

- **Emplacement :** `~/.local/share/tok/tracking.db` (Linux), `~/Library/Application Support/tok/tracking.db` (macOS)
- **Retention :** 90 jours automatique
- **Metriques :** tokens entree/sortie, pourcentage d'economies, temps d'execution, projet

---

### `tok gain` -- Statistiques d'economies

```bash
tok gain                        # Resume global
tok gain -p                     # Filtre par projet courant
tok gain --graph                # Graphe ASCII (30 derniers jours)
tok gain --history              # Historique recent des commandes
tok gain --daily                # Ventilation jour par jour
tok gain --weekly               # Ventilation par semaine
tok gain --monthly              # Ventilation par mois
tok gain --all                  # Toutes les ventilations
tok gain --quota -t pro         # Estimation d'economies sur le quota mensuel
tok gain --failures             # Log des echecs de parsing (commandes en fallback)
tok gain --top 25               # N commandes dans le tableau (defaut 10, max 100)
tok gain --rollup --top 25      # Agreger par outil (cargo, grep, git, toml:jq, …)
tok gain --by-client            # Ventilation par client (cursor, claude, …)
tok gain --format json          # Export JSON (pour dashboards)
tok gain --format csv           # Export CSV
```

**Options :**

| Option | Court | Description |
|--------|-------|-------------|
| `--project` | `-p` | Filtrer par repertoire courant |
| `--graph` | `-g` | Graphe ASCII des 30 derniers jours |
| `--history` | `-H` | Historique recent des commandes |
| `--quota` | `-q` | Estimation d'economies sur le quota mensuel |
| `--tier` | `-t` | Tier d'abonnement : `pro`, `5x`, `20x` (defaut: `20x`) |
| `--daily` | `-d` | Ventilation quotidienne |
| `--weekly` | `-w` | Ventilation hebdomadaire |
| `--monthly` | `-m` | Ventilation mensuelle |
| `--all` | `-a` | Toutes les ventilations |
| `--format` | `-f` | Format de sortie : `text`, `json`, `csv` |
| `--failures` | `-F` | Affiche les commandes en fallback |
| `--top` | | Nombre de commandes dans « By Command » (defaut 10, max 100) |
| `--rollup` | | Agreger par outil au lieu du `tok_cmd` complet |
| `--by-client` | | Ventilation par client (`TOK_CLIENT`) |

**Exemple de sortie :**
```
$ tok gain
TOK Token Savings Summary
  Total commands:     1,247
  Total input:        2,341,000 tokens
  Total output:       468,200 tokens
  Total saved:        1,872,800 tokens (80%)
  Avg per command:    1,501 tokens saved

Top commands:
  git status    312x  -82%
  cargo test    156x  -91%
  git diff       98x  -76%
```

---

### `tok discover` -- Opportunites manquees

**Objectif :** Analyse l'historique Claude Code pour trouver les commandes qui auraient pu etre optimisees par tok.

```bash
tok discover                          # Projet courant, 30 derniers jours
tok discover --all --since 7          # Tous les projets, 7 derniers jours
tok discover -p /chemin/projet        # Filtrer par projet
tok discover --limit 20              # Max commandes par section
tok discover --format json            # Export JSON
```

**Options :**

| Option | Court | Description |
|--------|-------|-------------|
| `--project` | `-p` | Filtrer par chemin de projet |
| `--limit` | `-l` | Max commandes par section (defaut: 15) |
| `--all` | `-a` | Scanner tous les projets |
| `--since` | `-s` | Derniers N jours (defaut: 30) |
| `--format` | `-f` | Format : `text`, `json` |

---

### `tok learn` -- Apprendre des erreurs

**Objectif :** Analyse l'historique d'erreurs CLI de Claude Code pour detecter les corrections recurrentes.

```bash
tok learn                             # Projet courant
tok learn --all --since 7             # Tous les projets
tok learn --write-rules               # Generer .claude/rules/cli-corrections.md
tok learn --min-confidence 0.8        # Seuil de confiance (defaut: 0.6)
tok learn --min-occurrences 3         # Occurrences minimales (defaut: 1)
tok learn --format json               # Export JSON
```

---

### `tok cc-economics` -- Analyse economique Claude Code

**Objectif :** Compare les depenses Claude Code (via ccusage) avec les economies TOK.

```bash
tok cc-economics                      # Resume
tok cc-economics --daily              # Ventilation quotidienne
tok cc-economics --weekly             # Ventilation hebdomadaire
tok cc-economics --monthly            # Ventilation mensuelle
tok cc-economics --all                # Toutes les ventilations
tok cc-economics --format json        # Export JSON
```

---

### `tok hook-audit` -- Metriques du hook

**Prerequis :** Necessite `TOK_HOOK_AUDIT=1` dans l'environnement.

```bash
tok hook-audit                        # 7 derniers jours (defaut)
tok hook-audit --since 30             # 30 derniers jours
tok hook-audit --since 0              # Tout l'historique
```

---

## Systeme de hooks

### Fonctionnement

Le hook TOK intercepte les commandes Bash dans Claude Code **avant leur execution** et les reecrit automatiquement en equivalent TOK.

**Flux :**
```
Claude Code "git status"
    |
    v
settings.json -> PreToolUse hook
    |
    v
tok-rewrite.sh (bash)
    |
    v
tok rewrite "git status"  ->  "tok git status"
    |
    v
Claude Code execute "tok git status"
    |
    v
Sortie filtree retournee a Claude (~10 tokens vs ~200)
```

**Points cles :**
- Claude ne voit jamais la recriture -- il recoit simplement une sortie optimisee
- Le hook est un delegateur leger (~50 lignes bash) qui appelle `tok rewrite`
- Toute la logique de recriture est dans le registre Rust (`src/discover/registry.rs`)
- Les commandes deja prefixees par `tok` passent sans modification
- Les heredocs (`<<`) ne sont pas modifies
- Les commandes non reconnues passent sans modification

### Installation

```bash
tok init -g                     # Installation recommandee (hook + TOK.md)
tok init -g --auto-patch        # Non-interactif (CI/CD)
tok init -g --hook-only         # Hook seul, sans TOK.md
tok init --show                 # Verifier l'installation
tok init -g --uninstall         # Desinstaller
```

### Fichiers installes

| Fichier | Description |
|---------|-------------|
| `~/.claude/hooks/tok-rewrite.sh` | Script hook (delegue a `tok rewrite`) |
| `~/.claude/TOK.md` | Instructions minimales pour le LLM |
| `~/.claude/settings.json` | Enregistrement du hook PreToolUse |

### `tok rewrite` -- Recriture de commande

Commande interne utilisee par le hook. Imprime la commande reecrite sur stdout (exit 0) ou sort avec exit 1 si aucun equivalent TOK n'existe.

```bash
tok rewrite "git status"           # -> "tok git status" (exit 0)
tok rewrite "terraform plan"       # -> (exit 1, pas de recriture)
tok rewrite "tok git status"       # -> "tok git status" (exit 0, inchange)
```

### `tok verify` -- Verification d'integrite

Verifie l'integrite du hook installe via un controle SHA-256.

```bash
tok verify
```

### Commandes reecrites automatiquement

| Commande brute | Reecrite en |
|----------------|-------------|
| `git status/diff/log/add/commit/push/pull` | `tok git ...` |
| `gh pr/issue/run` | `tok gh ...` |
| `cargo test/build/clippy/check` | `tok cargo ...` |
| `cat/head/tail <fichier>` | `tok read <fichier>` |
| `rg/grep <pattern>` | `tok grep <pattern>` |
| `ls` | `tok ls` |
| `tree` | `tok tree` |
| `wc` | `tok wc` |
| `vitest/jest` | `tok vitest run` |
| `tsc` | `tok tsc` |
| `eslint/biome` | `tok lint` |
| `prettier` | `tok prettier` |
| `playwright` | `tok playwright` |
| `prisma` | `tok prisma` |
| `ruff check/format` | `tok ruff ...` |
| `pytest` | `tok pytest` |
| `mypy` | `tok mypy` |
| `pip list/install` | `tok pip ...` |
| `go test/build/vet` | `tok go ...` |
| `golangci-lint` | `tok golangci-lint` |
| `docker ps/images/logs` | `tok docker ...` |
| `kubectl get/logs` | `tok kubectl ...` |
| `curl` | `tok curl` |
| `pnpm list/outdated` | `tok pnpm ...` |

### Exclusion de commandes

Pour empecher certaines commandes d'etre reecrites, ajoutez-les dans `config.toml` :

```toml
[hooks]
exclude_commands = ["curl", "playwright"]
```

---

## Configuration

### Fichier de configuration

**Emplacement :** `~/.config/tok/config.toml` (Linux) ou `~/Library/Application Support/tok/config.toml` (macOS)

**Commandes :**
```bash
tok config                # Afficher la configuration actuelle
tok config --create       # Creer le fichier avec les valeurs par defaut
```

### Structure complete

```toml
[tracking]
enabled = true              # Activer/desactiver le suivi
history_days = 90           # Jours de retention (nettoyage automatique)
database_path = "/custom/path/tracking.db"  # Chemin personnalise (optionnel)

[display]
colors = true               # Sortie coloree
emoji = true                # Utiliser les emojis
max_width = 120             # Largeur maximale de sortie

[filters]
ignore_dirs = [".git", "node_modules", "target", "__pycache__", ".venv", "vendor"]
ignore_files = ["*.lock", "*.min.js", "*.min.css"]

[tee]
enabled = true              # Activer la sauvegarde de sortie brute
mode = "failures"           # "failures" (defaut), "always", ou "never"
max_files = 20              # Rotation : garder les N derniers fichiers
# directory = "/custom/tee/path"  # Chemin personnalise (optionnel)

[telemetry]
enabled = true              # Telemetrie anonyme (1 ping/jour, opt-out possible)

[hooks]
exclude_commands = []       # Commandes a exclure de la recriture automatique
```

### Variables d'environnement

| Variable | Description |
|----------|-------------|
| `TOK_TEE_DIR` | Surcharge le repertoire tee |
| `TOK_TELEMETRY_DISABLED=1` | Desactiver la telemetrie |
| `TOK_HOOK_AUDIT=1` | Activer l'audit du hook |
| `SKIP_ENV_VALIDATION=1` | Desactiver la validation d'env (Next.js, etc.) |

---

## Systeme Tee

### Recuperation de sortie brute

Quand une commande echoue, TOK sauvegarde automatiquement la sortie brute complete dans un fichier log. Cela permet au LLM de lire la sortie sans re-executer la commande.

**Fonctionnement :**
1. La commande echoue (exit code != 0)
2. TOK sauvegarde la sortie brute dans `~/.local/share/tok/tee/`
3. Le chemin du fichier est affiche dans la sortie filtree
4. Le LLM peut lire le fichier si besoin de plus de details

**Sortie :**
```
FAILED: 2/15 tests
[full output: ~/.local/share/tok/tee/1707753600_cargo_test.log]
```

**Configuration :**

| Parametre | Defaut | Description |
|-----------|--------|-------------|
| `tee.enabled` | `true` | Activer/desactiver |
| `tee.mode` | `"failures"` | `"failures"`, `"always"`, `"never"` |
| `tee.max_files` | `20` | Rotation : garder les N derniers |
| Taille min | 500 octets | Les sorties trop courtes ne sont pas sauvegardees |
| Taille max fichier | 1 Mo | Troncature au-dela |

---

## Telemetrie

TOK envoie un ping anonyme une fois par jour (23h d'intervalle) pour des statistiques d'utilisation.

**Donnees envoyees :** hash de device, version, OS, architecture, nombre de commandes/24h, top commandes, pourcentage d'economies.

**Desactiver :**
```bash
# Via variable d'environnement
export TOK_TELEMETRY_DISABLED=1

# Via config.toml
[telemetry]
enabled = false
```

Aucune donnee personnelle, aucun contenu de commande, aucun chemin de fichier n'est transmis.

---

## Resume des economies par categorie

| Categorie | Commandes | Economies typiques |
|-----------|-----------|-------------------|
| **Fichiers** | ls, tree, read, find, grep, diff | 60-80% |
| **Git** | status, log, diff, show, add, commit, push, pull | 75-92% |
| **GitHub** | pr, issue, run, api | 26-87% |
| **Tests** | cargo test, vitest, playwright, pytest, go test | 90-99% |
| **Build/Lint** | cargo build, tsc, eslint, prettier, next, ruff, clippy | 70-87% |
| **Paquets** | pnpm, npm, pip, deps, prisma | 60-80% |
| **Conteneurs** | docker, kubectl | 70-80% |
| **Donnees** | json, env, log, curl, wget | 60-80% |
| **Analytique** | gain, discover, learn, cc-economics | N/A (meta) |

---

## Nombre total de commandes

TOK supporte **45+ commandes** reparties en 9 categories, avec passthrough automatique pour les sous-commandes non reconnues. Cela en fait un proxy universel : il est toujours sur a utiliser en prefixe.

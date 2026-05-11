//! Dispatches a parsed [`Cli`] to filter and passthrough command modules.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::{
    AgentTarget, CargoCommands, Cli, Commands, ComposeCommands, DockerCommands, DotnetCommands,
    GitCommands, GoCommands, GtCommands, HookCommands, KubectlCommands, PnpmCommands,
    PrismaCommands, PrismaMigrateCommands, VitestCommands,
};

use crate::cmds::cloud::{aws_cmd, container, curl_cmd, psql_cmd, wget_cmd};
use crate::cmds::dotnet::dotnet_cmd;
use crate::cmds::git::{diff_cmd, gh_cmd, git, gt_cmd};
use crate::cmds::go::{go_cmd, golangci_cmd};
use crate::cmds::js::{
    lint_cmd, next_cmd, npm_cmd, playwright_cmd, pnpm_cmd, prettier_cmd, prisma_cmd, tsc_cmd,
    vitest_cmd,
};
use crate::cmds::python::{mypy_cmd, pip_cmd, pytest_cmd, ruff_cmd};
use crate::cmds::ruby::{rake_cmd, rspec_cmd, rubocop_cmd};
use crate::cmds::rust::{cargo_cmd, runner};
use crate::cmds::system::{
    deps, env_cmd, find_cmd, format_cmd, grep_cmd, json_cmd, local_llm, log_cmd, ls, read, summary,
    tree, wc_cmd,
};

/// Run the subcommand selected on `cli` and return the process exit code.
pub(crate) fn dispatch(cli: Cli) -> Result<i32> {
    let code = match cli.command {
        Commands::Ls { args } => ls::run(&args, cli.verbose)?,

        Commands::Tree { args } => tree::run(&args, cli.verbose)?,

        // ISSUE #989: support multiple files (cat file1 file2 → tok read file1 file2)
        Commands::Read {
            files,
            level,
            max_lines,
            tail_lines,
            line_numbers,
        } => {
            let mut had_error = false;
            let mut stdin_seen = false;
            for file in &files {
                let result = if file == Path::new("-") {
                    if stdin_seen {
                        eprintln!("tok: warning: stdin specified more than once");
                        continue;
                    }
                    stdin_seen = true;
                    read::run_stdin(level, max_lines, tail_lines, line_numbers, cli.verbose)
                } else {
                    read::run(
                        file,
                        level,
                        max_lines,
                        tail_lines,
                        line_numbers,
                        cli.verbose,
                    )
                };
                if let Err(e) = result {
                    eprintln!("cat: {}: {}", file.display(), e.root_cause());
                    had_error = true;
                }
            }
            if had_error {
                1
            } else {
                0
            }
        }

        Commands::Smart {
            file,
            model,
            force_download,
        } => {
            local_llm::run(&file, &model, force_download, cli.verbose)?;
            0
        }

        Commands::Git {
            directory,
            config_override,
            git_dir,
            work_tree,
            no_pager,
            no_optional_locks,
            bare,
            literal_pathspecs,
            command,
        } => {
            // Build global git args (inserted between "git" and subcommand)
            let mut global_args: Vec<String> = Vec::new();
            for dir in &directory {
                global_args.push("-C".to_string());
                global_args.push(dir.clone());
            }
            for cfg in &config_override {
                global_args.push("-c".to_string());
                global_args.push(cfg.clone());
            }
            if let Some(ref dir) = git_dir {
                global_args.push("--git-dir".to_string());
                global_args.push(dir.clone());
            }
            if let Some(ref tree) = work_tree {
                global_args.push("--work-tree".to_string());
                global_args.push(tree.clone());
            }
            if no_pager {
                global_args.push("--no-pager".to_string());
            }
            if no_optional_locks {
                global_args.push("--no-optional-locks".to_string());
            }
            if bare {
                global_args.push("--bare".to_string());
            }
            if literal_pathspecs {
                global_args.push("--literal-pathspecs".to_string());
            }

            match command {
                GitCommands::Diff { args } => git::run(
                    git::GitCommand::Diff,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Log { args } => {
                    git::run(git::GitCommand::Log, &args, None, cli.verbose, &global_args)?
                }
                GitCommands::Status { args } => git::run(
                    git::GitCommand::Status,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Show { args } => git::run(
                    git::GitCommand::Show,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Add { args } => {
                    git::run(git::GitCommand::Add, &args, None, cli.verbose, &global_args)?
                }
                GitCommands::Commit { args } => git::run(
                    git::GitCommand::Commit,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Push { args } => git::run(
                    git::GitCommand::Push,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Pull { args } => git::run(
                    git::GitCommand::Pull,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Branch { args } => git::run(
                    git::GitCommand::Branch,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Fetch { args } => git::run(
                    git::GitCommand::Fetch,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Stash { subcommand, args } => git::run(
                    git::GitCommand::Stash { subcommand },
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Worktree { args } => git::run(
                    git::GitCommand::Worktree,
                    &args,
                    None,
                    cli.verbose,
                    &global_args,
                )?,
                GitCommands::Other(args) => git::run_passthrough(&args, &global_args, cli.verbose)?,
            }
        }

        Commands::Gh { subcommand, args } => {
            gh_cmd::run(&subcommand, &args, cli.verbose, cli.ultra_compact)?
        }

        Commands::Aws { subcommand, args } => aws_cmd::run(&subcommand, &args, cli.verbose)?,

        Commands::Psql { args } => psql_cmd::run(&args, cli.verbose)?,

        Commands::Pnpm { command } => match command {
            PnpmCommands::List { depth, args } => {
                pnpm_cmd::run(pnpm_cmd::PnpmCommand::List { depth }, &args, cli.verbose)?
            }
            PnpmCommands::Outdated { args } => {
                pnpm_cmd::run(pnpm_cmd::PnpmCommand::Outdated, &args, cli.verbose)?
            }
            PnpmCommands::Install { packages, args } => pnpm_cmd::run(
                pnpm_cmd::PnpmCommand::Install { packages },
                &args,
                cli.verbose,
            )?,
            PnpmCommands::Build { args } => {
                let mut build_args: Vec<String> = vec!["build".into()];
                build_args.extend(args);
                let os_args: Vec<OsString> = build_args.into_iter().map(OsString::from).collect();
                pnpm_cmd::run_passthrough(&os_args, cli.verbose)?
            }
            PnpmCommands::Typecheck { args } => tsc_cmd::run(&args, cli.verbose)?,
            PnpmCommands::Other(args) => pnpm_cmd::run_passthrough(&args, cli.verbose)?,
        },

        Commands::Err { command } => {
            let cmd = command.join(" ");
            runner::run_err(&cmd, cli.verbose)?
        }

        Commands::Test { command } => {
            let cmd = command.join(" ");
            runner::run_test(&cmd, cli.verbose)?
        }

        Commands::Json {
            file,
            depth,
            schema,
        } => {
            if file == Path::new("-") {
                json_cmd::run_stdin(depth, schema, cli.verbose)?;
            } else {
                json_cmd::run(&file, depth, schema, cli.verbose)?;
            }
            0
        }

        Commands::Deps { path } => {
            deps::run(&path, cli.verbose)?;
            0
        }

        Commands::Env { filter, show_all } => {
            env_cmd::run(filter.as_deref(), show_all, cli.verbose)?;
            0
        }

        Commands::Find { args } => {
            find_cmd::run_from_args(&args, cli.verbose)?;
            0
        }

        Commands::Diff { file1, file2 } => {
            if let Some(f2) = file2 {
                diff_cmd::run(&file1, &f2, cli.verbose)?;
            } else {
                diff_cmd::run_stdin(cli.verbose)?;
            }
            0
        }

        Commands::Log { file } => {
            if let Some(f) = file {
                log_cmd::run_file(&f, cli.verbose)?;
            } else {
                log_cmd::run_stdin(cli.verbose)?;
            }
            0
        }

        Commands::Dotnet { command } => match command {
            DotnetCommands::Build { args } => dotnet_cmd::run_build(&args, cli.verbose)?,
            DotnetCommands::Test { args } => dotnet_cmd::run_test(&args, cli.verbose)?,
            DotnetCommands::Restore { args } => dotnet_cmd::run_restore(&args, cli.verbose)?,
            DotnetCommands::Format { args } => dotnet_cmd::run_format(&args, cli.verbose)?,
            DotnetCommands::Other(args) => dotnet_cmd::run_passthrough(&args, cli.verbose)?,
        },

        Commands::Docker { command } => match command {
            DockerCommands::Ps => {
                container::run(container::ContainerCmd::DockerPs, &[], cli.verbose)?
            }
            DockerCommands::Images => {
                container::run(container::ContainerCmd::DockerImages, &[], cli.verbose)?
            }
            DockerCommands::Logs { container: c } => {
                container::run(container::ContainerCmd::DockerLogs, &[c], cli.verbose)?
            }
            DockerCommands::Compose { command: compose } => match compose {
                ComposeCommands::Ps => container::run_compose_ps(cli.verbose)?,
                ComposeCommands::Logs { service } => {
                    container::run_compose_logs(service.as_deref(), cli.verbose)?
                }
                ComposeCommands::Build { service } => {
                    container::run_compose_build(service.as_deref(), cli.verbose)?
                }
                ComposeCommands::Other(args) => {
                    container::run_compose_passthrough(&args, cli.verbose)?
                }
            },
            DockerCommands::Other(args) => container::run_docker_passthrough(&args, cli.verbose)?,
        },

        Commands::Kubectl { command } => match command {
            KubectlCommands::Pods { namespace, all } => {
                let mut args: Vec<String> = Vec::new();
                if all {
                    args.push("-A".to_string());
                } else if let Some(n) = namespace {
                    args.push("-n".to_string());
                    args.push(n);
                }
                container::run(container::ContainerCmd::KubectlPods, &args, cli.verbose)?
            }
            KubectlCommands::Services { namespace, all } => {
                let mut args: Vec<String> = Vec::new();
                if all {
                    args.push("-A".to_string());
                } else if let Some(n) = namespace {
                    args.push("-n".to_string());
                    args.push(n);
                }
                container::run(container::ContainerCmd::KubectlServices, &args, cli.verbose)?
            }
            KubectlCommands::Logs { pod, container: c } => {
                let mut args = vec![pod];
                if let Some(cont) = c {
                    args.push("-c".to_string());
                    args.push(cont);
                }
                container::run(container::ContainerCmd::KubectlLogs, &args, cli.verbose)?
            }
            KubectlCommands::Other(args) => container::run_kubectl_passthrough(&args, cli.verbose)?,
        },

        Commands::Summary { command } => {
            let cmd = command.join(" ");
            summary::run(&cmd, cli.verbose)?
        }

        Commands::Grep {
            pattern,
            path,
            max_len,
            max,
            context_only,
            file_type,
            line_numbers: _, // no-op: line numbers always enabled in grep_cmd::run
            extra_args,
        } => grep_cmd::run(
            &pattern,
            &path,
            max_len,
            max,
            context_only,
            file_type.as_deref(),
            &extra_args,
            cli.verbose,
        )?,

        Commands::Init {
            global,
            opencode,
            gemini,
            agent,
            show,
            claude_md,
            hook_only,
            auto_patch,
            no_patch,
            uninstall,
            codex,
            copilot,
            all,
        } => {
            if all {
                crate::hooks::init::run_all(cli.verbose)?;
            } else if show {
                crate::hooks::init::show_config(codex)?;
            } else if uninstall {
                let cursor = agent == Some(AgentTarget::Cursor);
                crate::hooks::init::uninstall(global, gemini, codex, cursor, cli.verbose)?;
            } else if gemini {
                let patch_mode = if auto_patch {
                    crate::hooks::init::PatchMode::Auto
                } else if no_patch {
                    crate::hooks::init::PatchMode::Skip
                } else {
                    crate::hooks::init::PatchMode::Ask
                };
                crate::hooks::init::run_gemini(global, hook_only, patch_mode, cli.verbose)?;
            } else if copilot {
                crate::hooks::init::run_copilot(cli.verbose)?;
            } else {
                let install_opencode = opencode;
                let install_claude = !opencode;
                let install_cursor = agent == Some(AgentTarget::Cursor);
                let install_windsurf = agent == Some(AgentTarget::Windsurf);
                let install_cline = agent == Some(AgentTarget::Cline);

                let patch_mode = if auto_patch {
                    crate::hooks::init::PatchMode::Auto
                } else if no_patch {
                    crate::hooks::init::PatchMode::Skip
                } else {
                    crate::hooks::init::PatchMode::Ask
                };
                crate::hooks::init::run(
                    global,
                    install_claude,
                    install_opencode,
                    install_cursor,
                    install_windsurf,
                    install_cline,
                    claude_md,
                    hook_only,
                    codex,
                    patch_mode,
                    cli.verbose,
                )?;
            }
            0
        }

        Commands::Wget { url, output, args } => {
            if output.as_deref() == Some("-") {
                wget_cmd::run_stdout(&url, &args, cli.verbose)?
            } else {
                // Pass -O <file> through to wget via args
                let mut all_args = Vec::new();
                if let Some(out_file) = &output {
                    all_args.push("-O".to_string());
                    all_args.push(out_file.clone());
                }
                all_args.extend(args);
                wget_cmd::run(&url, &all_args, cli.verbose)?
            }
        }

        Commands::Wc { args } => wc_cmd::run(&args, cli.verbose)?,

        Commands::Gain {
            project, // added
            graph,
            history,
            quota,
            tier,
            daily,
            weekly,
            monthly,
            all,
            format,
            failures,
            reset,
        } => {
            crate::analytics::gain::run(
                project, // added: pass project flag
                graph,
                history,
                quota,
                &tier,
                daily,
                weekly,
                monthly,
                all,
                &format,
                failures,
                reset,
                cli.verbose,
            )?;
            0
        }

        Commands::CcEconomics {
            daily,
            weekly,
            monthly,
            all,
            format,
        } => {
            crate::analytics::cc_economics::run(daily, weekly, monthly, all, &format, cli.verbose)?;
            0
        }

        Commands::Config { create } => {
            if create {
                let path = crate::core::config::Config::create_default()?;
                println!("Created: {}", path.display());
            } else {
                crate::core::config::show_config()?;
            }
            0
        }

        Commands::Vitest { command } => match command {
            VitestCommands::Run { args } => {
                vitest_cmd::run(vitest_cmd::VitestCommand::Run, &args, cli.verbose)?
            }
        },

        Commands::Prisma { command } => match command {
            PrismaCommands::Generate { args } => {
                prisma_cmd::run(prisma_cmd::PrismaCommand::Generate, &args, cli.verbose)?
            }
            PrismaCommands::Migrate { command } => match command {
                PrismaMigrateCommands::Dev { name, args } => prisma_cmd::run(
                    prisma_cmd::PrismaCommand::Migrate {
                        subcommand: prisma_cmd::MigrateSubcommand::Dev { name },
                    },
                    &args,
                    cli.verbose,
                )?,
                PrismaMigrateCommands::Status { args } => prisma_cmd::run(
                    prisma_cmd::PrismaCommand::Migrate {
                        subcommand: prisma_cmd::MigrateSubcommand::Status,
                    },
                    &args,
                    cli.verbose,
                )?,
                PrismaMigrateCommands::Deploy { args } => prisma_cmd::run(
                    prisma_cmd::PrismaCommand::Migrate {
                        subcommand: prisma_cmd::MigrateSubcommand::Deploy,
                    },
                    &args,
                    cli.verbose,
                )?,
            },
            PrismaCommands::DbPush { args } => {
                prisma_cmd::run(prisma_cmd::PrismaCommand::DbPush, &args, cli.verbose)?
            }
        },

        Commands::Tsc { args } => tsc_cmd::run(&args, cli.verbose)?,

        Commands::Next { args } => next_cmd::run(&args, cli.verbose)?,

        Commands::Lint { args } => lint_cmd::run(&args, cli.verbose)?,

        Commands::Prettier { args } => prettier_cmd::run(&args, cli.verbose)?,

        Commands::Format { args } => format_cmd::run(&args, cli.verbose)?,

        Commands::Playwright { args } => playwright_cmd::run(&args, cli.verbose)?,

        Commands::Cargo { command } => match command {
            CargoCommands::Build { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Build, &args, cli.verbose)?
            }
            CargoCommands::Test { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Test, &args, cli.verbose)?
            }
            CargoCommands::Clippy { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Clippy, &args, cli.verbose)?
            }
            CargoCommands::Check { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Check, &args, cli.verbose)?
            }
            CargoCommands::Install { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Install, &args, cli.verbose)?
            }
            CargoCommands::Nextest { args } => {
                cargo_cmd::run(cargo_cmd::CargoCommand::Nextest, &args, cli.verbose)?
            }
            CargoCommands::Other(args) => cargo_cmd::run_passthrough(&args, cli.verbose)?,
        },

        Commands::Npm { args } => npm_cmd::run(&args, cli.verbose, cli.skip_env)?,

        Commands::Curl { args } => curl_cmd::run(&args, cli.verbose)?,

        Commands::Discover {
            project,
            limit,
            all,
            since,
            format,
        } => {
            crate::discover::run(project.as_deref(), all, since, limit, &format, cli.verbose)?;
            0
        }

        Commands::Session {} => {
            crate::analytics::session_cmd::run(cli.verbose)?;
            0
        }

        Commands::Learn {
            project,
            all,
            since,
            format,
            write_rules,
            min_confidence,
            min_occurrences,
        } => {
            crate::learn::run(
                project,
                all,
                since,
                format,
                write_rules,
                min_confidence,
                min_occurrences,
            )?;
            0
        }

        Commands::Npx { args } => {
            if args.is_empty() {
                anyhow::bail!("npx requires a command argument");
            }

            // Intelligent routing: delegate to specialized filters
            match args[0].as_str() {
                "tsc" | "typescript" => tsc_cmd::run(&args[1..], cli.verbose)?,
                "eslint" => lint_cmd::run(&args[1..], cli.verbose)?,
                "prisma" => {
                    // Route to prisma_cmd based on subcommand
                    if args.len() > 1 {
                        let prisma_args: Vec<String> = args[2..].to_vec();
                        match args[1].as_str() {
                            "generate" => prisma_cmd::run(
                                prisma_cmd::PrismaCommand::Generate,
                                &prisma_args,
                                cli.verbose,
                            )?,
                            "db" if args.len() > 2 && args[2] == "push" => prisma_cmd::run(
                                prisma_cmd::PrismaCommand::DbPush,
                                &args[3..],
                                cli.verbose,
                            )?,
                            _ => {
                                // Passthrough other prisma subcommands
                                let timer = crate::core::tracking::TimedExecution::start();
                                let mut cmd = crate::core::utils::resolved_command("npx");
                                for arg in &args {
                                    cmd.arg(arg);
                                }
                                let status = cmd.status().context("Failed to run npx prisma")?;
                                let args_str = args.join(" ");
                                timer.track_passthrough(
                                    &format!("npx {}", args_str),
                                    &format!("tok npx {} (passthrough)", args_str),
                                );
                                crate::core::utils::exit_code_from_status(&status, "npx prisma")
                            }
                        }
                    } else {
                        let timer = crate::core::tracking::TimedExecution::start();
                        let status = crate::core::utils::resolved_command("npx")
                            .arg("prisma")
                            .status()
                            .context("Failed to run npx prisma")?;
                        timer.track_passthrough("npx prisma", "tok npx prisma (passthrough)");
                        crate::core::utils::exit_code_from_status(&status, "npx prisma")
                    }
                }
                "next" => next_cmd::run(&args[1..], cli.verbose)?,
                "prettier" => prettier_cmd::run(&args[1..], cli.verbose)?,
                "playwright" => playwright_cmd::run(&args[1..], cli.verbose)?,
                _ => {
                    // Generic passthrough with npm boilerplate filter
                    npm_cmd::run(&args, cli.verbose, cli.skip_env)?
                }
            }
        }

        Commands::Ruff { args } => ruff_cmd::run(&args, cli.verbose)?,

        Commands::Pytest { args } => pytest_cmd::run(&args, cli.verbose)?,

        Commands::Mypy { args } => mypy_cmd::run(&args, cli.verbose)?,

        Commands::Rake { args } => rake_cmd::run(&args, cli.verbose)?,

        Commands::Rubocop { args } => rubocop_cmd::run(&args, cli.verbose)?,

        Commands::Rspec { args } => rspec_cmd::run(&args, cli.verbose)?,

        Commands::Pip { args } => pip_cmd::run(&args, cli.verbose)?,

        Commands::Go { command } => match command {
            GoCommands::Test { args } => go_cmd::run_test(&args, cli.verbose)?,
            GoCommands::Build { args } => go_cmd::run_build(&args, cli.verbose)?,
            GoCommands::Vet { args } => go_cmd::run_vet(&args, cli.verbose)?,
            GoCommands::Other(args) => go_cmd::run_other(&args, cli.verbose)?,
        },

        Commands::Gt { command } => match command {
            GtCommands::Log { args } => gt_cmd::run_log(&args, cli.verbose)?,
            GtCommands::Submit { args } => gt_cmd::run_submit(&args, cli.verbose)?,
            GtCommands::Sync { args } => gt_cmd::run_sync(&args, cli.verbose)?,
            GtCommands::Restack { args } => gt_cmd::run_restack(&args, cli.verbose)?,
            GtCommands::Create { args } => gt_cmd::run_create(&args, cli.verbose)?,
            GtCommands::Branch { args } => gt_cmd::run_branch(&args, cli.verbose)?,
            GtCommands::Other(args) => gt_cmd::run_other(&args, cli.verbose)?,
        },

        Commands::GolangciLint { args } => golangci_cmd::run(&args, cli.verbose)?,

        Commands::HookAudit { since } => {
            crate::hooks::hook_audit_cmd::run(since, cli.verbose)?;
            0
        }

        Commands::Hook { command } => {
            match command {
                HookCommands::Gemini => crate::hooks::hook_cmd::run_gemini()?,
                HookCommands::Copilot => crate::hooks::hook_cmd::run_copilot()?,
            }
            0
        }

        Commands::Rewrite { args } => {
            let cmd = args.join(" ");
            crate::hooks::rewrite_cmd::run(&cmd)?;
            0
        }

        Commands::Proxy { args } => {
            use std::io::{Read, Write};
            use std::process::Stdio;
            use std::sync::atomic::{AtomicU32, Ordering};
            use std::thread;

            if args.is_empty() {
                anyhow::bail!(
                    "proxy requires a command to execute\nUsage: tok proxy <command> [args...]"
                );
            }

            let timer = crate::core::tracking::TimedExecution::start();

            // If a single quoted arg contains spaces, split it respecting quotes (#388).
            // e.g. tok proxy 'head -50 file.php' → cmd=head, args=["-50", "file.php"]
            // e.g. tok proxy 'git log --format="%H %s"' → cmd=git, args=["log", "--format=%H %s"]
            let (cmd_name, cmd_args): (String, Vec<String>) = if args.len() == 1 {
                let full = args[0].to_string_lossy();
                let parts = crate::discover::lexer::shell_split(&full);
                if parts.len() > 1 {
                    (parts[0].clone(), parts[1..].to_vec())
                } else {
                    (full.into_owned(), vec![])
                }
            } else {
                (
                    args[0].to_string_lossy().into_owned(),
                    args[1..]
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .collect(),
                )
            };

            if cli.verbose > 0 {
                eprintln!("Proxy mode: {} {}", cmd_name, cmd_args.join(" "));
            }

            // ISSUE #897: Kill proxy child on SIGINT/SIGTERM to prevent orphan
            // processes. Drop-based ChildGuard doesn't run on signals with
            // panic=abort, so we register a signal handler that kills the child
            // PID stored in this atomic.
            static PROXY_CHILD_PID: AtomicU32 = AtomicU32::new(0);

            #[cfg(unix)]
            {
                unsafe extern "C" fn handle_signal(sig: libc::c_int) {
                    let pid = PROXY_CHILD_PID.load(Ordering::SeqCst);
                    if pid != 0 {
                        libc::kill(pid as libc::pid_t, libc::SIGTERM);
                        libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), 0);
                    }
                    // Re-raise with default handler so parent sees correct exit status
                    libc::signal(sig, libc::SIG_DFL);
                    libc::raise(sig);
                }
                unsafe {
                    libc::signal(libc::SIGINT, handle_signal as libc::sighandler_t);
                    libc::signal(libc::SIGTERM, handle_signal as libc::sighandler_t);
                }
            }

            struct ChildGuard(Option<std::process::Child>);
            impl Drop for ChildGuard {
                fn drop(&mut self) {
                    if let Some(mut child) = self.0.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    PROXY_CHILD_PID.store(0, Ordering::SeqCst);
                }
            }

            let mut child = ChildGuard(Some(
                crate::core::utils::resolved_command(cmd_name.as_ref())
                    .args(&cmd_args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .context(format!("Failed to execute command: {}", cmd_name))?,
            ));

            // Store child PID for signal handler before anything can fail
            if let Some(ref inner) = child.0 {
                PROXY_CHILD_PID.store(inner.id(), Ordering::SeqCst);
            }

            let inner = child.0.as_mut().context("Child process missing")?;
            let stdout_pipe = inner
                .stdout
                .take()
                .context("Failed to capture child stdout")?;
            let stderr_pipe = inner
                .stderr
                .take()
                .context("Failed to capture child stderr")?;

            const CAP: usize = 1_048_576;

            let stdout_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
                let mut reader = stdout_pipe;
                let mut captured = Vec::new();
                let mut buf = [0u8; 8192];

                loop {
                    let count = reader.read(&mut buf)?;
                    if count == 0 {
                        break;
                    }
                    if captured.len() < CAP {
                        let take = count.min(CAP - captured.len());
                        captured.extend_from_slice(&buf[..take]);
                    }
                    let mut out = std::io::stdout().lock();
                    out.write_all(&buf[..count])?;
                    out.flush()?;
                }

                Ok(captured)
            });

            let stderr_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
                let mut reader = stderr_pipe;
                let mut captured = Vec::new();
                let mut buf = [0u8; 8192];

                loop {
                    let count = reader.read(&mut buf)?;
                    if count == 0 {
                        break;
                    }
                    if captured.len() < CAP {
                        let take = count.min(CAP - captured.len());
                        captured.extend_from_slice(&buf[..take]);
                    }
                    let mut err = std::io::stderr().lock();
                    err.write_all(&buf[..count])?;
                    err.flush()?;
                }

                Ok(captured)
            });

            let status = child
                .0
                .take()
                .context("Child process missing")?
                .wait()
                .context(format!("Failed waiting for command: {}", cmd_name))?;

            let stdout_bytes = stdout_handle
                .join()
                .map_err(|_| anyhow::anyhow!("stdout streaming thread panicked"))??;
            let stderr_bytes = stderr_handle
                .join()
                .map_err(|_| anyhow::anyhow!("stderr streaming thread panicked"))??;

            let stdout = String::from_utf8_lossy(&stdout_bytes);
            let stderr = String::from_utf8_lossy(&stderr_bytes);
            let full_output = format!("{}{}", stdout, stderr);

            // If security is enabled, apply obfuscation to the tracked output
            let security_enabled = cli.security
                || (!cli.no_security && {
                    let cfg = crate::core::config::Config::load().unwrap_or_default();
                    cfg.security.enabled
                });

            if security_enabled {
                let cfg = crate::core::config::Config::load().unwrap_or_default();
                let mode = crate::security::config::resolve_security_mode(
                    &cfg.security,
                    cli.security_mode.as_deref(),
                );
                let sec_result =
                    crate::security::process_input(&full_output, &cfg.security, &cfg.slm, mode);

                timer.track(
                    &format!("{} {}", cmd_name, cmd_args.join(" ")),
                    &format!("tok proxy {} {}", cmd_name, cmd_args.join(" ")),
                    &full_output,
                    &sec_result.sanitized_text,
                );

                if cli.verbose > 0 && sec_result.findings_count > 0 {
                    let report = crate::security::report::build_report(
                        &mode.to_string(),
                        &sec_result.map,
                        0,
                    );
                    eprintln!("\n{}", crate::security::report::format_report(&report));
                }
            } else {
                // Track usage (input = output since no filtering)
                timer.track(
                    &format!("{} {}", cmd_name, cmd_args.join(" ")),
                    &format!("tok proxy {} {}", cmd_name, cmd_args.join(" ")),
                    &full_output,
                    &full_output,
                );
            }

            crate::core::utils::exit_code_from_status(&status, &cmd_name)
        }

        Commands::Trust { list } => {
            crate::hooks::trust::run_trust(list)?;
            0
        }

        Commands::Untrust => {
            crate::hooks::trust::run_untrust()?;
            0
        }

        Commands::Verify {
            filter,
            require_all,
        } => {
            if filter.is_some() {
                // Filter-specific mode: run only that filter's tests
                crate::hooks::verify_cmd::run(filter, require_all)?;
            } else {
                // Default or --require-all: always run integrity check first
                crate::hooks::integrity::run_verify(cli.verbose)?;
                crate::hooks::verify_cmd::run(None, require_all)?;
            }
            0
        }

        Commands::Mem { command } => crate::cmds::mem::dispatch_mem(command)?,

        Commands::Forgemap { command } => {
            crate::cmds::forgemap::dispatch_forgemap(command, cli.verbose)?
        }

        Commands::SecurityInspect { file, report } => {
            use crate::security;
            let config = crate::core::config::Config::load().unwrap_or_default();
            let mode = security::config::resolve_security_mode(
                &config.security,
                cli.security_mode.as_deref(),
            );

            let text = if file == Path::new("-") {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                std::fs::read_to_string(&file)
                    .context(format!("Failed to read file: {}", file.display()))?
            };

            let findings = security::scanner::scan(&text, &config.security);

            if report || cli.verbose > 0 {
                let classified =
                    security::policy::classify(&findings, mode, &config.security.actions);
                let overall = security::policy::overall_severity(&classified);

                println!("{}", "TOK Security Inspect".bold());
                println!();
                println!("  Mode:     {}", mode);
                println!("  Findings: {}", findings.len());
                println!("  Severity: {:?}", overall);
                println!();

                for (i, cf) in classified.iter().enumerate() {
                    println!(
                        "  {}. [{}] {:?} (confidence: {:.0}%, action: {:?})",
                        i + 1,
                        format!("{:?}", cf.finding.entity_type).to_lowercase(),
                        "***REDACTED***",
                        cf.finding.confidence * 100.0,
                        cf.action,
                    );
                }
            } else {
                println!("{} finding(s) detected", findings.len());
            }
            0
        }

        Commands::Doctor { slm } => {
            if slm {
                let config = crate::core::config::Config::load().unwrap_or_default();
                crate::security::slm::doctor::run(&config.slm)?;
            } else {
                println!("tok doctor: use --slm to check SLM runtime health");
            }
            0
        }
        Commands::Man { filter } => {
            crate::print_manual(&filter);
            0
        }
    };
    Ok(code)
}

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

use crate::{io, match_runner};

pub type MatchId = i64;
pub type ProgramId = i64;

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct Config {
    pub container_name_prefix: String,
    pub cache_dir: std::path::PathBuf,
    pub match_run_dir: std::path::PathBuf,
    pub template_dir: std::collections::HashMap<Language, std::path::PathBuf>,
    pub compilation_timeout: std::time::Duration,
    pub agent_container_timeout: std::time::Duration,
    pub container_stdio_limit_bytes: usize,
    pub match_dir_cleanup: Option<MatchDirCleanup>,
}

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct MatchDirCleanup {
    pub period: std::time::Duration,
    pub staleness_threshold: std::time::Duration,
    pub max_per_iteration: usize,
}

pub struct Manager {
    config: Config,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub enum Language {
    Rust,
    Cpp,
    Java,
    Python,
    Go,
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub id: ProgramId,
    pub language: Language,
    pub param: String,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub id: ProgramId,
    pub language: Language,
    pub source_code: Vec<u8>,
}

#[derive(Debug)]
pub struct MatchConfig {
    pub config: match_runner::Config,
    pub id: MatchId,
    pub agents: Vec<Agent>,
}

impl MatchConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        let Some(_) = self.agents.first() else {
            return Err(anyhow!("No agents specified"));
        };
        Ok(())
    }
}

pub async fn run_match(s: Arc<Manager>, mc: MatchConfig) -> anyhow::Result<FullMatchResult> {
    mc.validate()?;
    let id = mc.id;
    let container_ids = (0..mc.agents.len())
        .map(|i| s.container_id(id, i))
        .collect::<Vec<_>>();
    let mr = run_match_impl(s.clone(), mc).await;
    for cid in container_ids.into_iter() {
        let local_manager = s.clone();
        tokio::task::spawn(async move { local_manager.kill_container(cid).await });
    }
    let fmr = match mr {
        Ok(r) => r,
        Err(e) => FullMatchResult {
            start_time: None,
            end_time: None,
            result: Err(debug_string(e)),
            log: Err("no log since match did not start".to_string()),
        },
    };
    let metadata = MatchMetadata {
        start_time: fmr.start_time,
        end_time: fmr.end_time,
        result: fmr.result.clone(),
    };
    let _ = s.store_metadata(id, metadata).await.inspect_err(|e| {
        log::error!("Failed to persist metadata for match {id}: {e:?}");
    });
    Ok(fmr)
}

async fn run_match_impl(s: Arc<Manager>, mc: MatchConfig) -> anyhow::Result<FullMatchResult> {
    let match_dir = s.match_dir(mc.id);
    let _ = delete_dir_if_safe(&match_dir).await;
    tokio::fs::create_dir_all(&match_dir)
        .await
        .context(format!("Failed to create match dir {match_dir:?}"))?;
    let mut container_ids = Vec::with_capacity(mc.agents.len());
    let mut ios = Vec::with_capacity(mc.agents.len());
    for (i, agent) in mc.agents.iter().enumerate() {
        let io = s.agent_io_for_match(mc.id, i);
        ios.push(io);
        io::create(&ios[i]).context("Failed to create io files for {io:?}")?;
        let container_id = s.container_id(mc.id, i);
        container_ids.push(container_id.clone());
        let mut command = tokio::process::Command::new("docker");
        command
            .args(["create", "--rm", "--name", &container_id])
            .args(s.docker_security_args())
            .args(s.docker_mount_io_args(&ios[i]))
            .args(s.docker_bot_resources_args())
            .args(["--workdir", "/agent"])
            .args([
                s.docker_bot_image_name(),
                "ash",
                "-c",
                &s.full_command(agent),
            ]);
        log::trace!("Running {command:?}");
        let output = command.output().await?;
        // TODO : cleanup all the created containers.
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to create container {container_id}; {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        s.copy_into_container(
            &container_id,
            s.compilation_cache_path(agent.id),
            "/agent/agent",
        )
        .await?;
    }
    let game_log_sink = s.log_sink(mc.id).await?;

    #[allow(clippy::unnecessary_to_owned)]
    for cid in container_ids.iter().cloned() {
        let s = s.clone();
        tokio::spawn(async move {
            s.start_container(&cid, s.config.agent_container_timeout)
                .await
        });
    }
    let params = mc.agents.into_iter().map(|a| a.param).collect();
    let start_time = time::OffsetDateTime::now_utc();
    let mr = match_runner::run(match_runner::MatchConfig {
        config: mc.config,
        ios,
        params,
        game_log_sink,
    })
    .await;
    let end_time = time::OffsetDateTime::now_utc();
    let log = s.get_log(mc.id).await.map_err(debug_string);
    let result = mr.map_err(debug_string);
    Ok(FullMatchResult {
        start_time: Some(start_time),
        end_time: Some(end_time),
        result,
        log,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum MatchResultError {
    RunMatchError(String),
    ReadMetadataError(String),
    ParseMetadataError(String),
}

impl std::fmt::Display for MatchResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MatchResultError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FullMatchResult {
    pub start_time: Option<time::OffsetDateTime>,
    pub end_time: Option<time::OffsetDateTime>,
    pub result: Result<match_runner::MatchResult, String>,
    pub log: Result<Vec<u8>, String>,
}

// Internal storage format for match metadata avoid usign in APIs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MatchMetadata {
    result: Result<match_runner::MatchResult, String>,
    start_time: Option<time::OffsetDateTime>,
    end_time: Option<time::OffsetDateTime>,
}

impl Manager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn get_result(&self, match_id: MatchId) -> Result<FullMatchResult, MatchResultError> {
        let metadata_filepath = self.metadata_file_path(match_id);
        let metadata_content = tokio::fs::read_to_string(&metadata_filepath)
            .await
            .map_err(|e| MatchResultError::ReadMetadataError(debug_string(e)))?;
        let metadata: MatchMetadata = toml::from_str(&metadata_content)
            .map_err(|e| MatchResultError::ParseMetadataError(debug_string(e)))?;
        let result = metadata.result.map_err(debug_string);
        let log = self.get_log(match_id).await.map_err(debug_string);
        Ok(FullMatchResult {
            start_time: metadata.start_time,
            end_time: metadata.end_time,
            result,
            log,
        })
    }

    pub async fn is_program_cached(self: &Manager, program_id: ProgramId) -> bool {
        tokio::fs::try_exists(self.compilation_cache_path(program_id))
            .await
            .unwrap_or(false)
    }

    pub async fn compile(&self, program: Program) -> anyhow::Result<()> {
        log::trace!("Compiling {:?}", program.id);
        if !self.needs_compilation(program.language) {
            let dir = self.compilation_cache_path(program.id);
            if !tokio::fs::try_exists(&dir)
                .await
                .context("Failed to check for presense of dir")?
            {
                tokio::fs::create_dir(&dir)
                    .await
                    .context("Failed to create compilation cache dir")?;
            }
            let artifact = dir.join(self.source_filename(program.language));
            tokio::fs::write(&artifact, &program.source_code)
                .await
                .context("Failed to write out source file to compilation cache")?;
            return Ok(());
        }
        let container_name = format!(
            "{}compile-{}",
            self.config.container_name_prefix, program.id
        );
        let compilation_command =
            format!("cd agent && {}", self.compilation_command(program.language));
        let mut command = tokio::process::Command::new("docker");
        command
            .args(["create", "--name", &container_name, "--workdir", "/agent"])
            .args(self.docker_compilation_resources_args())
            .args(self.docker_security_args())
            .args([
                self.docker_compilation_image_name(),
                "ash",
                "-c",
                compilation_command.as_str(),
            ]);
        log::trace!("Running {command:?}");
        let output = command
            .output()
            .await
            .context("Failed to create compilation container")?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to create container {container_name}; {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        let compile_result = self.compile_in_container(&container_name, &program).await;
        let _ = self
            .delete_container(&container_name)
            .await
            .map_err(|e| log::error!("Failed to delete container {container_name}: {e}"));
        compile_result
    }

    async fn store_metadata(
        &self,
        match_id: MatchId,
        metadata: MatchMetadata,
    ) -> anyhow::Result<()> {
        let filepath = self.metadata_file_path(match_id);
        tokio::fs::write(filepath, toml::to_string(&metadata)?.as_bytes()).await?;
        Ok(())
    }

    async fn get_log(&self, match_id: MatchId) -> anyhow::Result<Vec<u8>> {
        let filepath = self.log_file_path(match_id);
        tokio::fs::read(&filepath)
            .await
            .context(format!("Failed to read log file at {filepath:?}"))
    }

    fn compilation_cache_path(&self, id: ProgramId) -> std::path::PathBuf {
        self.config.cache_dir.join(format!("{id}"))
    }

    fn source_filename(&self, language: Language) -> std::path::PathBuf {
        match language {
            Language::Rust => "main.rs",
            Language::Python => "main.py",
            Language::Java => "Main.java",
            Language::Cpp => "main.cc",
            Language::Go => "main.go",
        }
        .into()
    }

    fn needs_compilation(&self, language: Language) -> bool {
        match language {
            Language::Cpp | Language::Go | Language::Java | Language::Rust => true,
            Language::Python => false,
        }
    }

    async fn copy_into_container(
        &self,
        container_name: &str,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let mut command = tokio::process::Command::new("docker");
        command.args([
            "cp",
            &from.as_ref().display().to_string(),
            &format!("{container_name}:{}", to.as_ref().display()),
        ]);
        log::trace!("Running {command:?}");
        let output = command.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to copy {:?} into container {container_name}; {:?}\nstdout:\n{}\nstderr:\n{}",
                from.as_ref(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        Ok(())
    }

    async fn copy_from_container(
        &self,
        container_name: &str,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let mut command = tokio::process::Command::new("docker");
        command.args([
            "cp",
            &format!("{container_name}:{}", from.as_ref().display()),
            &to.as_ref().display().to_string(),
        ]);
        log::trace!("Running {command:?}");
        let output = command.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to copy {:?} from container {container_name}; {:?}\nstdout:\n{}\nstderr:\n{}",
                from.as_ref(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        Ok(())
    }

    async fn compile_in_container(
        &self,
        container_name: &str,
        program: &Program,
    ) -> anyhow::Result<()> {
        if let Some(template_dir) = self.config.template_dir.get(&program.language) {
            self.copy_into_container(container_name, template_dir, "/agent/agent")
                .await
                .context("Failed to copy the template into container")?;
        }
        // TODO: async-tempfile.
        let td = tempfile::tempdir().context("Failed to create a temporary directory")?;
        tokio::fs::create_dir(td.path().join("agent"))
            .await
            .context("Failed to create a staging 'agent' directory")?;
        let project_dir = td.path().join("agent");
        let source_file = project_dir.join(self.source_filename(program.language));
        tokio::fs::write(&source_file, &program.source_code)
            .await
            .context("Failed to write source code into temp")?;
        self.copy_into_container(container_name, &project_dir, "/agent")
            .await
            .context("Failed to copy source file into container")?;
        self.start_container(container_name, self.config.compilation_timeout)
            .await?;
        let output_dir = self.compilation_cache_path(program.id);
        let _ = delete_dir_if_safe(&output_dir).await;

        let artifact_relative = self.compilation_artifact(program.language);
        tokio::fs::create_dir(&output_dir)
            .await
            .context("Failed to create the output dir for build artifacts")?;
        self.copy_from_container(
            container_name,
            PathBuf::from("/agent/agent".to_owned()).join(&artifact_relative),
            &output_dir,
        )
        .await?;
        Ok(())
    }

    fn compilation_command(&self, language: Language) -> String {
        match language {
            Language::Rust => "rustc --edition=2021 -O main.rs".to_owned(),
            Language::Go => "go build main.go".to_owned(),
            Language::Java => "javac Main.java".to_owned(),
            Language::Cpp => "g++ -std=c++23 -o main -O2 main.cc".to_owned(),
            Language::Python => "true".to_owned(),
        }
    }

    fn full_command(&self, agent: &Agent) -> String {
        format!(
            "cd agent && exec {} < /in > /out 2> /dev/null",
            self.run_command_for_agent(agent),
        )
    }

    fn run_command_for_agent(&self, agent: &Agent) -> String {
        match agent.language {
            Language::Rust | Language::Go | Language::Cpp => "./main",
            Language::Java => "java Main",
            Language::Python => "python3 main.py",
        }
        .to_owned()
    }

    fn compilation_artifact(&self, language: Language) -> PathBuf {
        match language {
            Language::Rust | Language::Go | Language::Cpp => "main".into(),
            Language::Java => "Main.class".into(),
            Language::Python => "main.py".into(),
        }
    }

    fn container_id(&self, match_id: MatchId, player_index: usize) -> String {
        format!(
            "{}match-{match_id}-agent-{player_index}",
            self.config.container_name_prefix
        )
    }

    async fn start_container(&self, container_name: &str, timeout: Duration) -> anyhow::Result<()> {
        let mut command = tokio::process::Command::new("docker");
        command
            .args(["start", "--interactive", "--attach", container_name])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        log::trace!("Running {command:?}");
        let mut child = command.spawn().context(format!(
            "Failed to spawn 'docker start' for {container_name}"
        ))?;
        let limit_bytes = self.config.container_stdio_limit_bytes;
        let stdout_join_handle = std::mem::take(&mut child.stdout).map(|stdout| {
            tokio::task::spawn(async move { io::read_with_limit(stdout, limit_bytes).await })
        });
        let stderr_join_handle = std::mem::take(&mut child.stderr).map(|stderr| {
            tokio::task::spawn(async move { io::read_with_limit(stderr, limit_bytes).await })
        });
        let (ok, mut overall_status) = match tokio::time::timeout(timeout, child.wait()).await {
            Err(_) => {
                let _ = child.kill().await.inspect_err(|e| {
                    log::error!("Failed to kill 'docker start' process for {container_name}: {e}");
                });
                (false, format!("Timeout ({:?})", timeout))
            }
            Ok(Err(e)) => (false, format!("{e:?}")),
            Ok(Ok(exit_code)) => (exit_code.success(), format!("{exit_code}")),
        };
        let stdout = if let Some(stdout_join_handle) = stdout_join_handle {
            stdout_join_handle
                .await
                .unwrap_or_else(|e| format!("Failed to join stdout reader: {e:?}"))
        } else {
            "No stdout handle found when spawning".to_owned()
        };
        let stderr = if let Some(stderr_join_handle) = stderr_join_handle {
            stderr_join_handle
                .await
                .unwrap_or_else(|e| format!("Failed to join stderr reader: {e:?}"))
        } else {
            "No stderr handle found when spawning".to_owned()
        };
        overall_status.push_str("\nstdout:\n");
        overall_status.push_str(&stdout);
        overall_status.push_str("\nstderr:\n");
        overall_status.push_str(&stderr);
        if ok {
            Ok(())
        } else {
            Err(anyhow::Error::msg(overall_status))
        }
    }

    async fn delete_container(&self, container_name: &str) -> anyhow::Result<()> {
        let mut command = tokio::process::Command::new("docker");
        command.args(["rm", "--volumes", container_name]);
        log::trace!("Running {command:?}");
        let output = command.output().await?;
        if !output.status.success() {
            return Err(anyhow!(
                "Failed to delete container {container_name}; {:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        Ok(())
    }

    async fn kill_container(&self, container_name: String) {
        let mut command = tokio::process::Command::new("docker");
        command.args(["kill", &container_name]);
        let _ = command.output().await;
        let mut command = tokio::process::Command::new("docker");
        command.args(["rm", "--volumes", "--force", &container_name]);
        let _ = command.output().await;
    }

    fn match_dir(&self, id: MatchId) -> PathBuf {
        self.config.match_run_dir.join(id.to_string())
    }

    async fn log_sink(&self, match_id: MatchId) -> anyhow::Result<match_runner::TextLogSink> {
        let filepath = self.log_file_path(match_id);
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&filepath)
            .await
            .context(format!("Failed to create log sink at {filepath:?}"))?;
        Ok(Box::new(async_compression::tokio::write::GzipEncoder::new(
            file,
        )))
    }

    fn log_file_path(&self, match_id: MatchId) -> PathBuf {
        self.match_dir(match_id).join("log")
    }

    fn metadata_file_path(&self, match_id: MatchId) -> PathBuf {
        self.match_dir(match_id).join("metadata.toml")
    }

    fn agent_io_for_match(&self, match_id: MatchId, agent_index: usize) -> io::AgentIO {
        let dir = self.match_dir(match_id);
        io::AgentIO {
            their_stdin: dir.join(format!("i{agent_index}")),
            their_stdout: dir.join(format!("o{agent_index}")),
        }
    }

    fn docker_security_args(&self) -> impl IntoIterator<Item = impl AsRef<OsStr>> {
        ["--read-only", "--network=none", "--runtime", "runsc"]
    }

    fn docker_mount_io_args(
        &self,
        io: &io::AgentIO,
    ) -> impl IntoIterator<Item = impl AsRef<OsStr>> {
        let mut in_mount = OsString::new();
        in_mount.push(io.their_stdin.as_os_str());
        in_mount.push(":/in");
        let mut out_mount = OsString::new();
        out_mount.push(io.their_stdout.as_os_str());
        out_mount.push(":/out");
        [
            OsString::from("-v"),
            in_mount,
            OsString::from("-v"),
            out_mount,
        ]
    }

    fn docker_compilation_resources_args(&self) -> impl IntoIterator<Item = impl AsRef<OsStr>> {
        [
            "--cpus",
            "1.0",
            "--memory",
            "512M",
            "--pids-limit",
            "256",
            "--mount",
            "type=volume,destination=/agent,volume-opt=size=50M",
            "--mount",
            "type=volume,destination=/root/.cache,volume-opt=size=50M",
            "--mount",
            "type=volume,destination=/tmp,volume-opt=size=50M",
        ]
    }

    fn docker_compilation_image_name(&self) -> &str {
        "alpine-build"
    }

    fn docker_bot_resources_args(&self) -> impl IntoIterator<Item = impl AsRef<OsStr>> {
        [
            "--cpus",
            "0.3",
            "--memory",
            "128M",
            "--pids-limit",
            "100",
            "--mount",
            "type=volume,destination=/agent,volume-opt=size=10M",
        ]
    }

    fn docker_bot_image_name(&self) -> &str {
        "alpine-build"
    }

    pub async fn cleanup_matches_iteration(&self) -> anyhow::Result<()> {
        let Some(cfg) = self.config.match_dir_cleanup.as_ref() else {
            log::warn!("cleanup_matches_iteration called with None config. Skipping");
            return Ok(());
        };
        log::trace!(
            "Starting match dir cleanup in {:?}",
            self.config.match_run_dir
        );
        let mut read_dir = tokio::fs::read_dir(&self.config.match_run_dir).await?;
        let mut count = 0;
        let mut deleted = 0;
        while let Some(entry) = read_dir.next_entry().await? {
            let Ok(metadata) = entry.metadata().await.inspect_err(error_log) else {
                continue;
            };
            let Ok(modified) = metadata.modified().inspect_err(error_log) else {
                continue;
            };
            if modified + cfg.staleness_threshold > std::time::SystemTime::now() {
                continue;
            }
            count += 1;
            if count > cfg.max_per_iteration {
                continue;
            }
            log::trace!("Cleaning up match dir {entry:?}");
            if delete_dir_if_safe(entry.path())
                .await
                .inspect_err(|e| {
                    log::error!("Failed to remove dir {:?}: {e:?}", entry.path());
                })
                .is_ok()
            {
                deleted += 1;
            };
        }
        log::trace!("Done match dir cleanup, deleted {deleted} entries.");
        Ok(())
    }
}

fn debug_string<D: std::fmt::Debug>(d: D) -> String {
    format!("{d:?}")
}

fn error_log<E: std::fmt::Debug>(e: &E) {
    log::error!("{e:?}");
}

// Checks if the given directoryt is under one of the approved directories,
// and recursively removes it.
// Any recursive directory deletion should use this function as a layer of
// safety for running experiments with this code. We do not want to accidentally
// remove / or $HOME.
async fn delete_dir_if_safe(filepath: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    let Some(fps) = filepath.as_ref().to_str() else {
        return Err(anyhow!(
            "Non-utf8 filepath {:?}, refusing to delete",
            filepath.as_ref()
        ));
    };
    if !fps.contains("/prod/") && !fps.contains("/tmp/") {
        return Err(anyhow!("Refusing to delete anything outside */prod/*"));
    }
    tokio::fs::remove_dir_all(filepath).await?;
    Ok(())
}

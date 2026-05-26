use crate::app_definitions::{
    parse_definition, system_definition, validate_definition, validate_definition_paths,
    AppDefinition, CaptureSourceDefinition, CleanupTargetDefinition, DefinitionRegistry,
    KindDefinition, TargetDefinition,
};
use crate::backup;
use crate::capture::{self, CaptureSourceSpec, CaptureSpec};
use crate::handlers;
use crate::identity;
use crate::lock::{self, FileLock};
use crate::paths::Paths;
use crate::process;
use crate::profiles::{
    apply_defaults, build_profile_id, new_oauth_profile, new_profile, reject_unsafe_field_arg,
    set_dotted_field, validate_static_profile, Profile, ProfileStore,
};
use crate::redaction::redact_fields;
use crate::state::{append_history, ActiveProfile, ActiveState, ResolvedTarget};
use crate::state::{load_pending, remove_pending, write_pending, PendingSwitch};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand};
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;

#[derive(Debug, Parser)]
#[command(name = "any-switch", version, about = "Local profile/state switcher")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Apps {
        #[command(subcommand)]
        command: Option<AppsCommand>,
    },
    List {
        app: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Add(AddArgs),
    Edit {
        id: String,
    },
    Use(UseArgs),
    Status {
        app: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    RestoreTarget {
        app: String,
        backup_id: String,
        #[arg(long, short = 'y')]
        yes: bool,
        #[arg(long)]
        allow_running: bool,
        #[arg(long)]
        assume_app_stopped: bool,
    },
    Remove {
        id: String,
        #[arg(long, short = 'y')]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    Detach {
        app: String,
    },
    Doctor {
        app: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    ImportCurrent {
        app: String,
        name: String,
        #[arg(long, default_value = "auto")]
        kind: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        allow_running: bool,
        #[arg(long)]
        assume_app_stopped: bool,
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AppsCommand {
    Show {
        app: String,
        #[arg(long)]
        json: bool,
    },
    Export {
        app: String,
        #[arg(long, default_value = "system")]
        source: String,
        #[arg(long = "as")]
        as_kind: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    Validate {
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum BackupCommand {
    List {
        app: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Path,
}

#[derive(Debug, Args)]
struct AddArgs {
    app: String,
    name: String,
    #[arg(long)]
    id: Option<String>,
    #[arg(long)]
    kind: String,
    #[arg(long = "field")]
    fields: Vec<String>,
    #[arg(long = "secret-field")]
    secret_fields: Vec<String>,
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct UseArgs {
    id: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, short = 'y')]
    yes: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    allow_running: bool,
    #[arg(long)]
    assume_app_stopped: bool,
    #[arg(long)]
    accept_resolved_change: bool,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::discover()?;
    let startup_permission_warnings = if matches!(&cli.command, Command::Doctor { .. }) {
        Some(permission_warnings(&paths)?)
    } else {
        None
    };
    paths.ensure_layout()?;

    match cli.command {
        Command::Apps {
            command: Some(AppsCommand::Validate { path: Some(path) }),
        } => validate_app_definition_file(&paths, &path),
        Command::Apps { command } => {
            let registry = DefinitionRegistry::load(&paths)?;
            apps_command(&paths, &registry, command)
        }
        Command::List { app, json } => {
            let registry = DefinitionRegistry::load(&paths)?;
            list_command(&paths, &registry, app.as_deref(), json)
        }
        Command::Show { id, json } => {
            let registry = DefinitionRegistry::load(&paths)?;
            show_command(&paths, &registry, &id, json)
        }
        Command::Add(args) => {
            let registry = DefinitionRegistry::load(&paths)?;
            add_command(&paths, &registry, args)
        }
        Command::Edit { id } => {
            let registry = DefinitionRegistry::load(&paths)?;
            edit_command(&paths, &registry, &id)
        }
        Command::Use(args) => {
            let registry = DefinitionRegistry::load(&paths)?;
            use_command(&paths, &registry, args)
        }
        Command::Status { app, json } => {
            let registry = DefinitionRegistry::load(&paths)?;
            status_command(&paths, &registry, app.as_deref(), json)
        }
        Command::Backup { command } => match command {
            BackupCommand::List { app, json } => {
                let rows = backup::list_backups(&paths, app.as_deref())?;
                if json {
                    let rows = rows
                        .into_iter()
                        .map(|(app, backup_id)| json!({ "app": app, "backup_id": backup_id }))
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                } else {
                    for (app, backup_id) in rows {
                        println!("{app}\t{backup_id}");
                    }
                }
                Ok(())
            }
        },
        Command::RestoreTarget {
            app,
            backup_id,
            yes,
            allow_running,
            assume_app_stopped,
        } => restore_target_command(
            &paths,
            &DefinitionRegistry::load(&paths)?,
            &app,
            &backup_id,
            yes,
            allow_running,
            assume_app_stopped,
        ),
        Command::Remove { id, yes, force } => remove_command(&paths, &id, yes || force),
        Command::Detach { app } => {
            let registry = DefinitionRegistry::load(&paths)?;
            detach_command(&paths, &registry, &app)
        }
        Command::Doctor { app, json } => {
            let registry = DefinitionRegistry::load(&paths)?;
            doctor_command(
                &paths,
                &registry,
                app.as_deref(),
                startup_permission_warnings,
                json,
            )
        }
        Command::Config { command } => match command {
            ConfigCommand::Path => {
                println!("{}", paths.profiles_path().display());
                Ok(())
            }
        },
        Command::ImportCurrent {
            app,
            name,
            kind,
            id,
            allow_running,
            assume_app_stopped,
            yes,
        } => import_current_command(
            &paths,
            &DefinitionRegistry::load(&paths)?,
            ImportCurrentOptions {
                app: &app,
                name: &name,
                requested_kind: &kind,
                explicit_id: id.as_deref(),
                allow_running,
                assume_app_stopped,
                yes,
            },
        ),
    }
}

fn apps_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    command: Option<AppsCommand>,
) -> Result<()> {
    match command {
        None => {
            for (id, loaded) in registry.iter() {
                let kinds = loaded
                    .definition
                    .kinds
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("{id}\t{:?}\t{kinds}", loaded.source);
            }
            Ok(())
        }
        Some(AppsCommand::Show { app, json }) => {
            let loaded = registry.get(&app)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&loaded.definition)?);
            } else {
                println!("{}", serde_yaml::to_string(&loaded.definition)?);
            }
            Ok(())
        }
        Some(AppsCommand::Export {
            app,
            source,
            as_kind,
            output,
            force,
        }) => {
            let definition = match source.as_str() {
                "system" => system_definition(&app)?
                    .ok_or_else(|| anyhow!("AppNotFound: {app} has no system definition"))?,
                "resolved" => registry.get(&app)?.definition.clone(),
                _ => {
                    return Err(anyhow!(
                        "invalid --source {source}; expected system or resolved"
                    ))
                }
            };
            let text = if as_kind.as_deref() == Some("override") {
                format!(
                    "schema_version: 1\napp:\n  id: {}\n  display_name: \"{}\"\n  definition_version: 1\n# Override support is intentionally narrow in the MVP implementation.\n",
                    definition.app.id, definition.app.display_name
                )
            } else if let Some(as_kind) = as_kind {
                return Err(anyhow!("invalid --as {as_kind}; expected override"));
            } else {
                serde_yaml::to_string(&definition)?
            };
            if let Some(path) = output {
                if path.exists() && !force {
                    return Err(anyhow!("{} exists; pass --force", path.display()));
                }
                crate::paths::write_private(&path, text.as_bytes())?;
            } else {
                print!("{text}");
            }
            Ok(())
        }
        Some(AppsCommand::Validate { path }) => {
            if let Some(path) = path {
                validate_app_definition_file(paths, &path)?;
            } else {
                DefinitionRegistry::load(paths)?;
                println!("ok");
            }
            Ok(())
        }
    }
}

fn validate_app_definition_file(paths: &Paths, path: &Path) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let definition = parse_definition(&text)?;
    validate_definition(&definition)?;
    validate_definition_paths(&definition, paths)?;
    println!("ok\t{}", path.display());
    Ok(())
}

fn list_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    app: Option<&str>,
    as_json: bool,
) -> Result<()> {
    if let Some(app) = app {
        registry.get(app)?;
    }
    let store = ProfileStore::load(paths)?;
    let mut rows = Vec::new();
    for profile in store
        .profiles
        .iter()
        .filter(|profile| app.is_none_or(|app| app == profile.app))
    {
        let definition = registry.get(&profile.app)?.definition.clone();
        let kind = definition
            .kinds
            .get(&profile.kind)
            .ok_or_else(|| anyhow!("KindNotSupported"))?;
        let fields = redact_fields(&profile.fields, &kind.field_schema);
        rows.push(json!({
            "id": profile.id,
            "app": profile.app,
            "kind": profile.kind,
            "name": profile.name,
            "fields": fields,
            "identity": profile.identity
        }));
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for row in rows {
            println!(
                "{}\t{}\t{}\t{}",
                row["id"].as_str().unwrap_or_default(),
                row["app"].as_str().unwrap_or_default(),
                row["kind"].as_str().unwrap_or_default(),
                row["name"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn show_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    id: &str,
    as_json: bool,
) -> Result<()> {
    let store = ProfileStore::load(paths)?;
    let profile = store
        .find(id)
        .ok_or_else(|| anyhow!("ProfileNotFound: {id}"))?;
    let loaded = registry.get(&profile.app)?;
    let kind = loaded
        .definition
        .kinds
        .get(&profile.kind)
        .ok_or_else(|| anyhow!("KindNotSupported: {}", profile.kind))?;
    let out = json!({
        "id": profile.id,
        "app": profile.app,
        "kind": profile.kind,
        "name": profile.name,
        "created_at": profile.created_at,
        "fields": redact_fields(&profile.fields, &kind.field_schema),
        "identity": profile.identity
    });
    if as_json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("{}", serde_yaml::to_string(&out)?);
    }
    Ok(())
}

fn add_command(paths: &Paths, registry: &DefinitionRegistry, args: AddArgs) -> Result<()> {
    let _profiles_lock = FileLock::acquire(lock::profiles_lock(paths))?;
    let loaded = registry.get(&args.app)?;
    let kind = loaded
        .definition
        .kinds
        .get(&args.kind)
        .ok_or_else(|| anyhow!("KindNotSupported: {}", args.kind))?;
    if args.kind == "oauth_capture" {
        return Err(anyhow!(
            "oauth_capture profiles must be created by import-current"
        ));
    }
    let mut fields = IndexMap::new();
    apply_defaults(&mut fields, &kind.field_schema);
    for field in args.fields {
        let (key, value) = split_kv(&field)?;
        reject_unsafe_field_arg(key, &kind.field_schema)?;
        set_dotted_field(&mut fields, key, Value::String(value.to_string()));
    }
    for field in args.secret_fields {
        let (key, value_ref) = split_kv(&field)?;
        let value = read_secret_ref(paths, value_ref)?;
        set_dotted_field(&mut fields, key, Value::String(value));
    }
    let id = build_profile_id(&args.app, &args.name, args.id.as_deref())?;
    let profile = new_profile(&args.app, &args.kind, &args.name, id.clone(), fields);
    validate_static_profile(&loaded.definition, &profile)?;

    let mut store = ProfileStore::load(paths)?;
    if store.find(&id).is_some() {
        if !args.force {
            return Err(anyhow!("ProfileExists: {id}"));
        }
        let existing = store.find(&id).expect("checked existing profile").clone();
        if existing.app != args.app
            || existing.kind != args.kind
            || existing.kind == "oauth_capture"
        {
            return Err(anyhow!(
                "ProfileExists: --force can only replace same app/kind non-oauth profiles"
            ));
        }
        let _app_lock = FileLock::acquire(lock::app_lock(paths, &args.app)?)?;
        if let Some(pending) = load_pending(paths, &args.app)? {
            return Err(anyhow!(
                "InterruptedSwitch: {} has pending {} operation {} at stage {}",
                args.app,
                pending.operation,
                pending.operation_id,
                pending.stage
            ));
        }
        store.remove(&id);
    }
    store.profiles.push(profile);
    store.save(paths)?;
    println!("added {id}");
    Ok(())
}

fn edit_command(paths: &Paths, registry: &DefinitionRegistry, id: &str) -> Result<()> {
    let _profiles_lock = FileLock::acquire(lock::profiles_lock(paths))?;
    let mut store = ProfileStore::load(paths)?;
    let index = store
        .profiles
        .iter()
        .position(|profile| profile.id == id)
        .ok_or_else(|| anyhow!("ProfileNotFound: {id}"))?;
    let original = store.profiles[index].clone();
    let _app_lock = FileLock::acquire(lock::app_lock(paths, &original.app)?)?;
    if let Some(pending) = load_pending(paths, &original.app)? {
        return Err(anyhow!(
            "InterruptedSwitch: {} has pending {} operation {} at stage {}",
            original.app,
            pending.operation,
            pending.operation_id,
            pending.stage
        ));
    }

    let edit_dir = paths.state_dir().join("edit");
    crate::paths::ensure_dir_private(&edit_dir)?;
    let operation_id = uuid::Uuid::now_v7().to_string();
    let edit_path = edit_dir.join(format!("{operation_id}.yaml"));
    crate::paths::write_private(&edit_path, serde_yaml::to_string(&original)?.as_bytes())?;

    let edit_result = (|| -> Result<Profile> {
        run_editor(&edit_path)?;
        let edited_text = fs::read_to_string(&edit_path)
            .with_context(|| format!("read edited profile {}", edit_path.display()))?;
        let edited: Profile = serde_yaml::from_str(&edited_text)?;
        validate_edited_profile(&original, &edited)?;
        let loaded = registry.get(&edited.app)?;
        if edited.kind == "oauth_capture" {
            let _ = CaptureSpec::from_profile(&edited)?;
        } else {
            validate_static_profile(&loaded.definition, &edited)?;
        }
        Ok(edited)
    })();

    let _ = fs::remove_file(&edit_path);
    let edited = edit_result?;
    store.profiles[index] = edited.clone();
    store.save(paths)?;
    println!("edited {}", edited.id);
    Ok(())
}

fn run_editor(path: &std::path::Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| anyhow!("EditorNotConfigured: set VISUAL or EDITOR"))?;
    let status = ProcessCommand::new(&editor)
        .arg(path)
        .status()
        .with_context(|| format!("run editor {editor}"))?;
    if !status.success() {
        return Err(anyhow!("EditorFailed: {editor} exited with {status}"));
    }
    Ok(())
}

fn validate_edited_profile(original: &Profile, edited: &Profile) -> Result<()> {
    let changed = [
        ("id", original.id.as_str(), edited.id.as_str()),
        ("app", original.app.as_str(), edited.app.as_str()),
        ("kind", original.kind.as_str(), edited.kind.as_str()),
        (
            "created_at",
            original.created_at.as_str(),
            edited.created_at.as_str(),
        ),
    ]
    .into_iter()
    .find(|(_, before, after)| before != after);
    if let Some((field, _, _)) = changed {
        return Err(anyhow!("ImmutableFieldChanged: {field}"));
    }
    if original.schema_version != edited.schema_version {
        return Err(anyhow!("ImmutableFieldChanged: schema_version"));
    }
    Ok(())
}

enum ImportDraft {
    Static {
        kind: String,
        fields: IndexMap<String, Value>,
    },
    OAuth {
        identity: IndexMap<String, Value>,
        capture: CaptureSpec,
        files: Vec<(String, Vec<u8>)>,
    },
}

impl ImportDraft {
    fn kind(&self) -> &str {
        match self {
            Self::Static { kind, .. } => kind,
            Self::OAuth { .. } => "oauth_capture",
        }
    }
}

struct CapturedDefinitionSource {
    source: CaptureSourceSpec,
    bytes: Vec<u8>,
    identity: Option<IndexMap<String, Value>>,
}

struct ImportCurrentOptions<'a> {
    app: &'a str,
    name: &'a str,
    requested_kind: &'a str,
    explicit_id: Option<&'a str>,
    allow_running: bool,
    assume_app_stopped: bool,
    yes: bool,
}

fn import_current_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    options: ImportCurrentOptions<'_>,
) -> Result<()> {
    let app = options.app;
    let _profiles_lock = FileLock::acquire(lock::profiles_lock(paths))?;
    let _app_lock = FileLock::acquire(lock::app_lock(paths, app)?)?;
    let loaded = registry.get(app)?;
    let _target_locks =
        lock::acquire_target_locks(paths, import_current_target_ids(paths, &loaded.definition)?)?;
    ensure_definition_guards(paths, &loaded.definition)?;
    let mut drafts = detect_definition_import(paths, &loaded.definition)?;
    if options.requested_kind != "auto" {
        drafts.retain(|draft| draft.kind() == options.requested_kind);
    }
    if drafts.is_empty() {
        return Err(anyhow!(
            "TargetMissing: no importable current state for {app}"
        ));
    }
    if drafts.len() > 1 {
        let kinds = drafts
            .iter()
            .map(ImportDraft::kind)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "ImportAmbiguous: detected multiple current states for {app}: {kinds}; pass --kind"
        ));
    }

    let draft = drafts.remove(0);
    let process_warnings = enforce_process_rule(
        &loaded.definition,
        draft.kind() == "oauth_capture",
        options.allow_running,
        options.assume_app_stopped,
        options.yes,
    )?;
    let mut store = ProfileStore::load(paths)?;
    store.ensure_writable_schema()?;
    let requested_id = build_profile_id(app, options.name, options.explicit_id)?;
    let mut imported_id = requested_id.clone();
    let mut updated_existing = false;
    match draft {
        ImportDraft::Static { kind, fields } => {
            if store.find(&requested_id).is_some() {
                return Err(anyhow!("ProfileExists: {requested_id}"));
            }
            let profile = new_profile(app, &kind, options.name, requested_id.clone(), fields);
            validate_static_profile(&loaded.definition, &profile)?;
            store.profiles.push(profile);
            store.save(paths)?;
        }
        ImportDraft::OAuth {
            identity,
            capture,
            files,
        } => {
            if !required_identity_matches_definition(
                &loaded.definition,
                "oauth_capture",
                &identity,
                &identity,
            )? {
                return Err(anyhow!(
                    "IdentityMissing: imported {app} OAuth state is missing required identity fields"
                ));
            }
            let capture_value = capture.to_value()?;
            if let Some(index) = oauth_import_update_index(
                &store,
                &loaded.definition,
                app,
                &requested_id,
                &identity,
            )? {
                imported_id = store.profiles[index].id.clone();
                store.profiles[index].identity = identity;
                store.profiles[index].capture = Some(capture_value);
                capture::write_capture_files(paths, &imported_id, files, false)?;
                updated_existing = true;
            } else {
                let profile = new_oauth_profile(
                    app,
                    options.name,
                    requested_id.clone(),
                    identity,
                    capture_value,
                );
                store.profiles.push(profile);
                capture::write_capture_files(paths, &requested_id, files, false)?;
                imported_id = requested_id;
            }
            store.save(paths)?;
        }
    }
    {
        let imported_profile = store
            .find(&imported_id)
            .ok_or_else(|| anyhow!("ProfileNotFound: {imported_id}"))?;
        let imported_kind = loaded
            .definition
            .kinds
            .get(&imported_profile.kind)
            .ok_or_else(|| anyhow!("KindNotSupported: {}", imported_profile.kind))?;
        let resolved_targets = resolved_targets_from_paths(&resolve_target_paths(
            paths,
            imported_kind,
            imported_profile,
        )?);
        let _state_lock = lock::acquire_state_lock(paths)?;
        let mut state = ActiveState::load(paths)?;
        state.active_profiles.insert(
            app.to_string(),
            Some(ActiveProfile {
                id: imported_id.clone(),
                resolved_targets,
            }),
        );
        state.save(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": uuid::Uuid::now_v7().to_string(),
                "time": Utc::now(),
                "operation": "import-current",
                "app": app,
                "profile": imported_id,
                "kind": store.find(&imported_id).map(|profile| profile.kind.clone()),
                "updated_existing": updated_existing,
                "warnings": process_warnings,
                "ok": true
            }),
        )?;
    }
    if updated_existing {
        println!("updated {imported_id}");
    } else {
        println!("imported {imported_id}");
    }
    Ok(())
}

fn oauth_import_update_index(
    store: &ProfileStore,
    definition: &AppDefinition,
    app: &str,
    requested_id: &str,
    identity: &IndexMap<String, Value>,
) -> Result<Option<usize>> {
    let requested_index = store
        .profiles
        .iter()
        .position(|profile| profile.id == requested_id);
    let identity_index = store.profiles.iter().position(|profile| {
        profile.app == app
            && profile.kind == "oauth_capture"
            && required_identity_matches_definition(
                definition,
                "oauth_capture",
                &profile.identity,
                identity,
            )
            .unwrap_or(false)
    });
    match (requested_index, identity_index) {
        (Some(requested), Some(matched)) if requested != matched => Err(anyhow!(
            "ProfileExists: {requested_id} already exists for a different profile"
        )),
        (Some(index), _) => {
            let profile = &store.profiles[index];
            if profile.app != app || profile.kind != "oauth_capture" {
                return Err(anyhow!("ProfileExists: {requested_id}"));
            }
            if !required_identity_matches_definition(
                definition,
                "oauth_capture",
                &profile.identity,
                identity,
            )? {
                return Err(anyhow!(
                    "ProfileExists: {requested_id} has a different required identity"
                ));
            }
            Ok(Some(index))
        }
        (None, Some(index)) => Ok(Some(index)),
        (None, None) => Ok(None),
    }
}

fn import_current_target_ids(paths: &Paths, definition: &AppDefinition) -> Result<Vec<String>> {
    let mut target_ids = Vec::new();
    if let Some(kind) = definition.kinds.get("oauth_capture") {
        for source in &kind.capture_sources {
            match source.handler.as_str() {
                "file_capture" => {
                    if let Some(path) = &source.path {
                        target_ids.push(format!(
                            "file:{}",
                            paths.expand_target_path(path)?.display()
                        ));
                    }
                }
                "secret_entry" => {
                    let service = source.service.as_deref().ok_or_else(|| {
                        anyhow!("secret_entry source {} missing service", source.stored_as)
                    })?;
                    let account = source.account.as_deref().ok_or_else(|| {
                        anyhow!("secret_entry source {} missing account", source.stored_as)
                    })?;
                    let resolved_account =
                        account.replace("${MACOS_USER}", &crate::paths::current_os_user());
                    target_ids.push(format!(
                        "keychain:{}:{}:{}",
                        source.backend.as_deref().unwrap_or("macos_keychain"),
                        service,
                        resolved_account
                    ));
                }
                _ => {}
            }
        }
        for cleanup in &kind.cleanup_targets {
            target_ids.push(format!(
                "file:{}",
                paths.expand_target_path(&cleanup.path)?.display()
            ));
        }
        for target in &kind.targets {
            if matches!(
                target.handler.as_str(),
                "file_capture" | "json_subtree" | "toml_managed_paths"
            ) {
                target_ids.push(format!(
                    "file:{}",
                    paths.expand_target_path(&target.path)?.display()
                ));
            }
        }
    }
    target_ids.sort();
    target_ids.dedup();
    Ok(target_ids)
}

fn detect_definition_import(paths: &Paths, definition: &AppDefinition) -> Result<Vec<ImportDraft>> {
    let mut drafts = Vec::new();
    drafts.extend(detect_definition_static_imports(paths, definition)?);
    if let Some(draft) = detect_definition_oauth_import(paths, definition)? {
        drafts.push(draft);
    }
    if drafts.is_empty() {
        if let Some(err) = definition_import_unmatched_error(paths, definition)? {
            return Err(anyhow!(err));
        }
    }
    Ok(drafts)
}

fn definition_import_unmatched_error(
    paths: &Paths,
    definition: &AppDefinition,
) -> Result<Option<String>> {
    for kind in definition.kinds.values() {
        for target in &kind.targets {
            let Some(error) = target.import_unmatched_error.as_ref() else {
                continue;
            };
            let path = paths.expand_target_path(&target.path)?;
            if path.exists() {
                return Ok(Some(error.clone()));
            }
        }
    }
    Ok(None)
}

fn detect_definition_static_imports(
    paths: &Paths,
    definition: &AppDefinition,
) -> Result<Vec<ImportDraft>> {
    let mut drafts = Vec::new();
    for (kind_name, kind) in definition
        .kinds
        .iter()
        .filter(|(name, _)| name.as_str() != "oauth_capture")
    {
        let mut fields = IndexMap::new();
        let mut read_any = false;
        for target in &kind.targets {
            match target.handler.as_str() {
                "json_env_merge" => {
                    let path = paths.expand_target_path(&target.path)?;
                    if !path.exists() {
                        continue;
                    }
                    let Some(env) = handlers::read_json_path(
                        &path,
                        target.json_path.as_deref().unwrap_or("$.env"),
                    )?
                    else {
                        continue;
                    };
                    for (key, template) in &target.mapping {
                        let Some(field_path) = template_field_path(template) else {
                            continue;
                        };
                        if let Some(value) = env.get(key).and_then(Value::as_str) {
                            set_import_field(
                                &mut fields,
                                field_path,
                                Value::String(value.to_string()),
                            )?;
                            read_any = true;
                        }
                    }
                }
                "file_capture" => {
                    let path = paths.expand_target_path(&target.path)?;
                    if !path.exists() || target.import_json_fields.is_empty() {
                        continue;
                    }
                    let root: Value = serde_json::from_slice(&fs::read(&path)?)?;
                    if !import_json_requirements_match(&root, target)? {
                        continue;
                    }
                    import_json_forbidden_strings_absent(&root, target, &path)?;
                    for (field_path, json_path) in &target.import_json_fields {
                        if let Some(value) = value_at_simple_json_path(&root, json_path)? {
                            if value.as_str().is_some_and(|value| !value.is_empty()) {
                                set_import_field(&mut fields, field_path, value)?;
                                read_any = true;
                            }
                        }
                    }
                }
                "toml_managed_paths" => {
                    let path = paths.expand_target_path(&target.path)?;
                    if !path.exists() {
                        continue;
                    }
                    let doc = fs::read_to_string(&path)?.parse::<toml_edit::DocumentMut>()?;
                    for toml_path in &target.toml_paths {
                        if let Some(value) = toml_value_at_path(&doc, toml_path) {
                            set_import_field(&mut fields, toml_path, value)?;
                            read_any = true;
                        }
                    }
                }
                _ => {}
            }
        }
        if read_any {
            drafts.push(ImportDraft::Static {
                kind: kind_name.clone(),
                fields,
            });
        }
    }
    Ok(drafts)
}

fn detect_definition_oauth_import(
    paths: &Paths,
    definition: &AppDefinition,
) -> Result<Option<ImportDraft>> {
    let Some(kind) = definition.kinds.get("oauth_capture") else {
        return Ok(None);
    };
    let Some(identity_definition) = kind.identity.as_ref() else {
        return Ok(None);
    };
    let mut sources = Vec::new();
    let mut files = Vec::new();
    let mut root = Value::Object(Default::default());
    let mut used_names = BTreeSet::new();
    let mut read_any = false;
    for target in &kind.targets {
        let Some((source, bytes, identity_value)) =
            capture_definition_oauth_target(paths, target, &mut used_names)?
        else {
            continue;
        };
        merge_identity_root(&mut root, identity_value);
        files.push((source.stored_as.clone(), bytes));
        sources.push(source);
        read_any = true;
    }
    if !read_any {
        return Ok(None);
    }
    let identity = identity::extract_identity_from_definition(&root, identity_definition)?;
    let mut captured_required_source = kind.capture_sources.is_empty();
    for source_definition in &kind.capture_sources {
        let Some(captured) = capture_definition_oauth_source(paths, source_definition)? else {
            continue;
        };
        captured_required_source = true;
        if let Some(source_identity) = &captured.identity {
            ensure_source_identity_matches_definition(
                definition,
                "oauth_capture",
                &identity,
                source_identity,
                "SourceInconsistent",
            )?;
        }
        files.push((captured.source.stored_as.clone(), captured.bytes));
        sources.push(captured.source);
    }
    if !captured_required_source {
        return Ok(None);
    }
    Ok(Some(ImportDraft::OAuth {
        identity,
        capture: CaptureSpec { sources },
        files,
    }))
}

fn capture_definition_oauth_source(
    paths: &Paths,
    source: &CaptureSourceDefinition,
) -> Result<Option<CapturedDefinitionSource>> {
    if !definition_source_applies_to_current_platform(source) {
        return Ok(None);
    }
    let bytes = match source.handler.as_str() {
        "file_capture" => {
            let path = source
                .path
                .as_deref()
                .ok_or_else(|| anyhow!("file_capture source {} missing path", source.stored_as))?;
            let path = paths.expand_target_path(path)?;
            if !path.exists() {
                return Ok(None);
            }
            fs::read(path)?
        }
        "secret_entry" => {
            let service = source.service.as_deref().ok_or_else(|| {
                anyhow!("secret_entry source {} missing service", source.stored_as)
            })?;
            let account = source.account.as_deref().ok_or_else(|| {
                anyhow!("secret_entry source {} missing account", source.stored_as)
            })?;
            let resolved_account =
                account.replace("${MACOS_USER}", &crate::paths::current_os_user());
            let Ok(bytes) = crate::keychain::read_generic_password(service, &resolved_account)
            else {
                return Ok(None);
            };
            bytes
        }
        other => {
            return Err(anyhow!(
                "UnknownHandler: unsupported capture source {other}"
            ))
        }
    };
    let source_identity = if let Some(identity_definition) = &source.identity {
        let root: Value = serde_json::from_slice(&bytes)?;
        let identity = identity::extract_identity_from_definition(&root, identity_definition)?;
        (!identity.is_empty()).then_some(identity)
    } else {
        None
    };
    Ok(Some(CapturedDefinitionSource {
        source: CaptureSourceSpec {
            source_type: match source.handler.as_str() {
                "file_capture" => "file".to_string(),
                "secret_entry" => "secret_entry".to_string(),
                other => {
                    return Err(anyhow!(
                        "UnknownHandler: unsupported capture source {other}"
                    ))
                }
            },
            backend: source.backend.clone(),
            path: source.path.clone(),
            service: source.service.clone(),
            account: source.account.clone(),
            json_path: None,
            toml_paths: Vec::new(),
            stored_as: source.stored_as.clone(),
            required: source.required,
            platforms: source.platforms.clone(),
        },
        bytes,
        identity: source_identity,
    }))
}

fn definition_source_applies_to_current_platform(source: &CaptureSourceDefinition) -> bool {
    if source.platforms.is_empty() {
        return true;
    }
    let current = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    source.platforms.iter().any(|platform| platform == current)
}

fn capture_definition_oauth_target(
    paths: &Paths,
    target: &TargetDefinition,
    used_names: &mut BTreeSet<String>,
) -> Result<Option<(CaptureSourceSpec, Vec<u8>, Value)>> {
    match target.handler.as_str() {
        "file_capture" => {
            let path = paths.expand_target_path(&target.path)?;
            if !path.exists() {
                return Ok(None);
            }
            let bytes = fs::read(&path)?;
            let identity_value: Value = serde_json::from_slice(&bytes)?;
            if !import_json_requirements_match(&identity_value, target)? {
                return Ok(None);
            }
            import_json_forbidden_strings_absent(&identity_value, target, &path)?;
            let stored_as = unique_stored_as(
                used_names,
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("capture.json"),
            );
            Ok(Some((
                CaptureSourceSpec {
                    source_type: "file".to_string(),
                    backend: None,
                    path: Some(target.path.clone()),
                    service: None,
                    account: None,
                    json_path: None,
                    toml_paths: Vec::new(),
                    stored_as,
                    required: Some(true),
                    platforms: Vec::new(),
                },
                bytes,
                identity_value,
            )))
        }
        "json_subtree" => {
            let path = paths.expand_target_path(&target.path)?;
            if !path.exists() {
                return Ok(None);
            }
            let json_path = target
                .json_path
                .as_deref()
                .ok_or_else(|| anyhow!("json_subtree target missing json_path"))?;
            let Some(value) = handlers::read_json_path(&path, json_path)? else {
                return Ok(None);
            };
            let identity_value = value_wrapped_at_json_path(json_path, value.clone())?;
            let stored_as = unique_stored_as(
                used_names,
                &format!(
                    "{}.json",
                    json_path
                        .trim_start_matches("$.")
                        .rsplit('.')
                        .next()
                        .unwrap_or("subtree")
                ),
            );
            Ok(Some((
                CaptureSourceSpec {
                    source_type: "json_subtree".to_string(),
                    backend: None,
                    path: Some(target.path.clone()),
                    service: None,
                    account: None,
                    json_path: target.json_path.clone(),
                    toml_paths: Vec::new(),
                    stored_as,
                    required: Some(true),
                    platforms: Vec::new(),
                },
                serde_json::to_vec_pretty(&value)?,
                identity_value,
            )))
        }
        "toml_managed_paths" => {
            let path = paths.expand_target_path(&target.path)?;
            if !path.exists() {
                return Ok(None);
            }
            let stored_as = unique_stored_as(
                used_names,
                &format!(
                    "{}.managed.toml",
                    path.file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("config")
                ),
            );
            Ok(Some((
                CaptureSourceSpec {
                    source_type: "toml_managed_paths".to_string(),
                    backend: None,
                    path: Some(target.path.clone()),
                    service: None,
                    account: None,
                    json_path: None,
                    toml_paths: target.toml_paths.clone(),
                    stored_as,
                    required: Some(false),
                    platforms: Vec::new(),
                },
                handlers::capture_toml_fragment(&path, &target.toml_paths)?.into_bytes(),
                Value::Object(Default::default()),
            )))
        }
        _ => Ok(None),
    }
}

fn template_field_path(template: &str) -> Option<&str> {
    let trimmed = template.trim();
    let expr = trimmed.strip_prefix("{{")?.strip_suffix("}}")?.trim();
    expr.strip_prefix("fields.")
}

fn set_import_field(fields: &mut IndexMap<String, Value>, path: &str, value: Value) -> Result<()> {
    let mut parts = path.split('.').collect::<Vec<_>>();
    let Some(last) = parts.pop() else {
        return Err(anyhow!("ImportInvalid: empty field path"));
    };
    if parts.is_empty() {
        fields.insert(last.to_string(), value);
        return Ok(());
    }
    let first = parts.remove(0);
    let entry = fields
        .entry(first.to_string())
        .or_insert_with(|| Value::Object(Default::default()));
    let mut current = entry;
    for part in parts {
        let object = current
            .as_object_mut()
            .ok_or_else(|| anyhow!("ImportInvalid: field path conflict at {path}"))?;
        current = object
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    let object = current
        .as_object_mut()
        .ok_or_else(|| anyhow!("ImportInvalid: field path conflict at {path}"))?;
    object.insert(last.to_string(), value);
    Ok(())
}

fn import_json_requirements_match(root: &Value, target: &TargetDefinition) -> Result<bool> {
    for (path, expected) in &target.import_json_matches {
        if value_at_simple_json_path(root, path)?.as_ref() != Some(expected) {
            return Ok(false);
        }
    }
    for path in &target.import_json_required_strings {
        if value_at_simple_json_path(root, path)?
            .and_then(|value| value.as_str().map(str::to_string))
            .is_none_or(|value| value.is_empty())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn import_json_forbidden_strings_absent(
    root: &Value,
    target: &TargetDefinition,
    path: &Path,
) -> Result<()> {
    for json_path in &target.import_json_forbidden_strings {
        if value_at_simple_json_path(root, json_path)?
            .and_then(|value| value.as_str().map(str::to_string))
            .is_some_and(|value| !value.is_empty())
        {
            let matched_rule = target
                .import_json_matches
                .iter()
                .map(|(path, value)| {
                    let name = path.trim_start_matches("$.");
                    let value = value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_string);
                    format!("{name}={value}")
                })
                .collect::<Vec<_>>()
                .join(",");
            return Err(anyhow!(
                "ImportAmbiguous: {} has forbidden string {} for matching import rule {}",
                path.display(),
                json_path,
                matched_rule
            ));
        }
    }
    Ok(())
}

fn value_at_simple_json_path(root: &Value, path: &str) -> Result<Option<Value>> {
    crate::app_definitions::validate_simple_json_path(path)?;
    if path == "$" {
        return Ok(Some(root.clone()));
    }
    let mut current = root;
    for segment in path.trim_start_matches("$.").split('.') {
        let Some(next) = current.as_object().and_then(|object| object.get(segment)) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current.clone()))
}

fn toml_value_at_path(doc: &toml_edit::DocumentMut, path: &str) -> Option<Value> {
    let mut current = doc.as_item();
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    toml_item_to_json(current)
}

fn toml_item_to_json(item: &toml_edit::Item) -> Option<Value> {
    let value = item.as_value()?;
    if let Some(value) = value.as_str() {
        return Some(Value::String(value.to_string()));
    }
    if let Some(value) = value.as_integer() {
        return Some(Value::Number(value.into()));
    }
    if let Some(value) = value.as_bool() {
        return Some(Value::Bool(value));
    }
    Some(Value::String(value.to_string()))
}

fn value_wrapped_at_json_path(json_path: &str, value: Value) -> Result<Value> {
    crate::app_definitions::validate_simple_json_path(json_path)?;
    if json_path == "$" {
        return Ok(value);
    }
    let mut root = value;
    for segment in json_path.trim_start_matches("$.").split('.').rev() {
        let mut object = serde_json::Map::new();
        object.insert(segment.to_string(), root);
        root = Value::Object(object);
    }
    Ok(root)
}

fn unique_stored_as(used_names: &mut BTreeSet<String>, preferred: &str) -> String {
    let base = sanitize_stored_as(preferred);
    if used_names.insert(base.clone()) {
        return base;
    }
    for index in 1.. {
        let candidate = format!("{index}-{base}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("unbounded integer iterator should find a unique stored_as")
}

fn sanitize_stored_as(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '-'])
        .to_string();
    if sanitized.is_empty() || sanitized == "manifest.json" {
        "capture.json".to_string()
    } else {
        sanitized
    }
}

fn ensure_definition_guards(paths: &Paths, definition: &AppDefinition) -> Result<()> {
    for guard in &definition.guards {
        match guard.handler.as_str() {
            "toml_string_allowlist" => ensure_toml_string_allowlist_guard(paths, guard)?,
            other => return Err(anyhow!("UnknownHandler: {other}")),
        }
    }
    Ok(())
}

fn ensure_toml_string_allowlist_guard(
    paths: &Paths,
    guard: &crate::app_definitions::GuardDefinition,
) -> Result<()> {
    let path = paths.expand_target_path(&guard.path)?;
    if !path.exists() {
        if guard.missing_ok {
            return Ok(());
        }
        return Err(anyhow!("GuardFailed: missing {}", path.display()));
    }
    let text = fs::read_to_string(&path)?;
    let doc = text.parse::<toml_edit::DocumentMut>()?;
    let Some(toml_path) = guard.toml_path.as_deref() else {
        return Err(anyhow!(
            "DefinitionLoadFailed: toml_string_allowlist guard requires toml_path"
        ));
    };
    let Some(value) = toml_string_at_path(&doc, toml_path) else {
        if guard.missing_ok {
            return Ok(());
        }
        return Err(anyhow!(
            "GuardFailed: missing {} {}",
            path.display(),
            toml_path
        ));
    };
    if guard.allowed_values.iter().any(|allowed| allowed == value) {
        return Ok(());
    }
    let kind = guard.error_kind.as_deref().unwrap_or("GuardFailed");
    Err(anyhow!("{kind}: {} {toml_path}={value:?}", path.display()))
}

fn toml_string_at_path<'a>(doc: &'a toml_edit::DocumentMut, path: &str) -> Option<&'a str> {
    let mut current = doc.as_item();
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_str()
}

fn use_command(paths: &Paths, registry: &DefinitionRegistry, args: UseArgs) -> Result<()> {
    let initial_store = ProfileStore::load(paths)?;
    let app = initial_store
        .find(&args.id)
        .ok_or_else(|| anyhow!("ProfileNotFound: {}", args.id))?
        .app
        .clone();
    let _app_lock = FileLock::acquire(lock::app_lock(paths, &app)?)?;
    let store = ProfileStore::load(paths)?;
    let profile = store
        .find(&args.id)
        .ok_or_else(|| anyhow!("ProfileNotFound: {}", args.id))?;
    if profile.app != app {
        return Err(anyhow!(
            "ProfileChanged: {} changed app while acquiring lock",
            args.id
        ));
    }
    if let Some(pending) = load_pending(paths, &app)? {
        if !recover_pending(paths, registry, &store, &pending)? {
            return Err(anyhow!(
                "InterruptedSwitch: {} has pending {} operation {} at stage {}",
                app,
                pending.operation,
                pending.operation_id,
                pending.stage
            ));
        }
    }
    let loaded = registry.get(&profile.app)?;
    ensure_definition_guards(paths, &loaded.definition)?;
    let kind = loaded
        .definition
        .kinds
        .get(&profile.kind)
        .ok_or_else(|| anyhow!("KindNotSupported"))?;
    let mut state = ActiveState::load(paths)?;
    let previous_id = state
        .active_profiles
        .get(&profile.app)
        .and_then(|entry| entry.as_ref())
        .map(|entry| entry.id.clone());
    let previous_profile = previous_id
        .as_deref()
        .map(|id| {
            store
                .find(id)
                .ok_or_else(|| anyhow!("ProfileNotFound: {id}"))
        })
        .transpose()?;
    let oauth_io_required = profile.kind == "oauth_capture"
        || previous_profile
            .as_ref()
            .is_some_and(|previous| previous.kind == "oauth_capture");
    let process_warnings = enforce_process_rule(
        &loaded.definition,
        oauth_io_required,
        args.allow_running,
        args.assume_app_stopped,
        args.yes,
    )?;
    if profile.kind == "oauth_capture" {
        capture::ensure_capture_complete(paths, profile)?;
    }
    let stale_warning =
        oauth_stale_warning(paths, profile, store.preferences.oauth_stale_warn_days)?;
    let target_paths = resolve_target_paths(paths, kind, profile)?;
    let resolved_targets = resolved_targets_from_paths(&target_paths);
    let resolved_change_warning = state
        .active_profiles
        .get(&profile.app)
        .and_then(|entry| entry.as_ref())
        .and_then(|active| {
            if active.resolved_targets.is_empty() || active.resolved_targets == resolved_targets {
                None
            } else {
                Some(json!({
                    "type": "resolved_targets_changed",
                    "old": active.resolved_targets.clone(),
                    "new": resolved_targets.clone()
                }))
            }
        });
    if resolved_change_warning.is_some() && !args.accept_resolved_change {
        return Err(anyhow!(
            "ResolvedTargetChanged: resolved targets for {} changed; run import-current or pass --accept-resolved-change",
            profile.app
        ));
    }
    let backup_inputs =
        backup_inputs_for_profile(paths, kind, profile, &target_paths, oauth_io_required)?;
    let target_ids = backup_inputs
        .iter()
        .map(backup::BackupInput::target_id)
        .collect::<Vec<_>>();
    let _target_locks = lock::acquire_target_locks(paths, target_ids)?;

    if args.dry_run {
        let writeback_enabled = previous_profile
            .as_ref()
            .is_some_and(|previous| previous.kind == "oauth_capture");
        let writeback_profile = previous_profile
            .as_ref()
            .filter(|previous| previous.kind == "oauth_capture")
            .map(|previous| {
                json!({
                    "id": previous.id,
                    "kind": previous.kind,
                    "identity": previous.identity,
                })
            });
        let writeback_only =
            previous_id.as_deref() == Some(profile.id.as_str()) && profile.kind == "oauth_capture";
        let backup_targets = if writeback_only {
            Vec::new()
        } else {
            backup_inputs
                .iter()
                .map(backup_input_plan)
                .collect::<Vec<_>>()
        };
        let plan = json!({
            "app": profile.app,
            "profile": profile.id,
            "kind": profile.kind,
            "writeback": writeback_enabled,
            "writeback_profile": writeback_profile,
            "writeback_only": writeback_only,
            "defensive_backup": {
                "enabled": !writeback_only,
                "targets": backup_targets,
            },
            "post_write_verify": post_write_verify_plan(&loaded.definition, profile, &target_paths)?,
            "targets": target_paths.iter().map(|(_, path)| path.display().to_string()).collect::<Vec<_>>(),
            "resolved_targets": resolved_targets,
            "identity": profile.identity,
            "warnings": combined_warnings(process_warnings, resolved_change_warning, stale_warning.clone(), Vec::new()),
            "secrets": "***"
        });
        if args.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!("{}", serde_yaml::to_string(&plan)?);
        }
        return Ok(());
    }
    if !args.yes {
        return Err(anyhow!("use requires --yes in this build"));
    }

    if previous_id.as_deref() == Some(profile.id.as_str()) && profile.kind == "oauth_capture" {
        writeback_oauth_profile(paths, &loaded.definition, profile)?;
        let _state_lock = lock::acquire_state_lock(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": uuid::Uuid::now_v7().to_string(),
                "time": Utc::now(),
                "operation": "use",
                "app": profile.app,
                "to_profile": profile.id,
                "kind": profile.kind,
                "no_op_apply": true,
                "writeback_ok": true,
                "warnings": process_warnings,
                "ok": true
            }),
        )?;
        println!("refreshed capture for {}", profile.id);
        return Ok(());
    }

    if let Some(previous) = previous_profile {
        if previous.kind == "oauth_capture" {
            writeback_oauth_profile(paths, &loaded.definition, previous)?;
        }
    }

    let backup_id = backup::create_backup_with_inputs(paths, &profile.app, &backup_inputs)?;
    let operation_id = uuid::Uuid::now_v7().to_string();
    write_pending(
        paths,
        &PendingSwitch {
            schema_version: 1,
            operation: "use".to_string(),
            operation_id: operation_id.clone(),
            app: profile.app.clone(),
            from_profile: previous_id.clone(),
            to_profile: Some(profile.id.clone()),
            backup_id: Some(backup_id.clone()),
            restore_from_backup_id: None,
            targets: resolved_targets.clone(),
            stage: "applying".to_string(),
            expected: json!({ "kind": profile.kind, "profile": profile.id }),
        },
    )?;
    let identity_warnings = match apply_and_verify_use(
        paths,
        &loaded.definition,
        profile,
        &target_paths,
        ApplyUseContext {
            operation_id: &operation_id,
            previous_id: previous_id.clone(),
            backup_id: &backup_id,
            resolved_targets: &resolved_targets,
        },
    ) {
        Ok(warnings) => warnings,
        Err(err) => {
            return rollback_failed_use(
                paths,
                FailedUseRollback {
                    profile,
                    previous_id: previous_id.clone(),
                    operation_id: &operation_id,
                    backup_id: &backup_id,
                    resolved_targets: &resolved_targets,
                    stage: "verifying",
                    err,
                },
            );
        }
    };
    write_pending(
        paths,
        &PendingSwitch {
            schema_version: 1,
            operation: "use".to_string(),
            operation_id: operation_id.clone(),
            app: profile.app.clone(),
            from_profile: previous_id.clone(),
            to_profile: Some(profile.id.clone()),
            backup_id: Some(backup_id.clone()),
            restore_from_backup_id: None,
            targets: resolved_targets.clone(),
            stage: "bookkeeping".to_string(),
            expected: json!({ "kind": profile.kind, "profile": profile.id }),
        },
    )?;
    backup::prune_backups(paths, &profile.app, store.preferences.keep_backups)?;
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        state = ActiveState::load(paths)?;
        state.active_profiles.insert(
            profile.app.clone(),
            Some(ActiveProfile {
                id: profile.id.clone(),
                resolved_targets,
            }),
        );
        state.save(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": operation_id,
                "time": Utc::now(),
                "operation": "use",
                "app": profile.app,
                "to_profile": profile.id,
                "kind": profile.kind,
                "backup_id": backup_id,
                "warnings": combined_warnings(process_warnings, resolved_change_warning, stale_warning, identity_warnings),
                "ok": true
            }),
        )?;
    }
    remove_pending(paths, &profile.app)?;
    println!(
        "switched {} to {} (backup {backup_id})",
        profile.app, profile.id
    );
    Ok(())
}

fn restore_target_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    app: &str,
    backup_id: &str,
    yes: bool,
    allow_running: bool,
    assume_app_stopped: bool,
) -> Result<()> {
    if !yes {
        return Err(anyhow!("restore-target requires --yes in this build"));
    }
    let _app_lock = FileLock::acquire(lock::app_lock(paths, app)?)?;
    if let Some(pending) = load_pending(paths, app)? {
        let store = ProfileStore::load(paths)?;
        if !recover_pending(paths, registry, &store, &pending)? {
            return Err(anyhow!(
                "InterruptedSwitch: {app} has pending {} operation {} at stage {}",
                pending.operation,
                pending.operation_id,
                pending.stage
            ));
        }
    }
    let loaded = registry.get(app)?;
    ensure_definition_guards(paths, &loaded.definition)?;
    let restore_manifest = backup::load_validated_manifest(paths, app, backup_id)?;
    let restore_inputs = backup::inputs_from_manifest(&restore_manifest)?;
    let _target_locks = lock::acquire_target_locks(
        paths,
        restore_inputs
            .iter()
            .map(backup::BackupInput::target_id)
            .collect(),
    )?;
    let oauth_io_required = backup_manifest_requires_app_stopped(paths, app, backup_id)?;
    let process_warnings = enforce_process_rule(
        &loaded.definition,
        oauth_io_required,
        allow_running,
        assume_app_stopped,
        yes,
    )?;
    let rollback_inputs = restore_inputs;
    let rollback_id = backup::create_backup_with_inputs(paths, app, &rollback_inputs)?;
    let operation_id = uuid::Uuid::now_v7().to_string();
    let resolved_targets = rollback_inputs
        .iter()
        .map(|input| ResolvedTarget {
            target_id: input.target_id(),
            resolved_path: input
                .resolved_path()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    write_pending(
        paths,
        &PendingSwitch {
            schema_version: 1,
            operation: "restore-target".to_string(),
            operation_id: operation_id.clone(),
            app: app.to_string(),
            from_profile: None,
            to_profile: None,
            backup_id: Some(rollback_id.clone()),
            restore_from_backup_id: Some(backup_id.to_string()),
            targets: resolved_targets.clone(),
            stage: "applying".to_string(),
            expected: json!({ "backup_id": backup_id }),
        },
    )?;
    if let Err(err) = backup::restore_backup(paths, app, backup_id) {
        return rollback_failed_restore(
            paths,
            FailedRestoreRollback {
                app,
                operation_id: &operation_id,
                restore_from_backup_id: backup_id,
                rollback_backup_id: &rollback_id,
                resolved_targets: &resolved_targets,
                stage: "applying",
                err,
            },
        );
    }
    write_pending(
        paths,
        &PendingSwitch {
            schema_version: 1,
            operation: "restore-target".to_string(),
            operation_id: operation_id.clone(),
            app: app.to_string(),
            from_profile: None,
            to_profile: None,
            backup_id: Some(rollback_id.clone()),
            restore_from_backup_id: Some(backup_id.to_string()),
            targets: resolved_targets.clone(),
            stage: "verifying".to_string(),
            expected: json!({ "backup_id": backup_id }),
        },
    )?;
    if !backup::live_matches_backup(paths, app, backup_id)? {
        return rollback_failed_restore(
            paths,
            FailedRestoreRollback {
                app,
                operation_id: &operation_id,
                restore_from_backup_id: backup_id,
                rollback_backup_id: &rollback_id,
                resolved_targets: &resolved_targets,
                stage: "verifying",
                err: anyhow!("VerifyFailed: live targets do not match backup {backup_id}"),
            },
        );
    }
    write_pending(
        paths,
        &PendingSwitch {
            schema_version: 1,
            operation: "restore-target".to_string(),
            operation_id: operation_id.clone(),
            app: app.to_string(),
            from_profile: None,
            to_profile: None,
            backup_id: Some(rollback_id.clone()),
            restore_from_backup_id: Some(backup_id.to_string()),
            targets: Vec::new(),
            stage: "bookkeeping".to_string(),
            expected: json!({ "backup_id": backup_id }),
        },
    )?;
    let store = ProfileStore::load(paths)?;
    backup::prune_backups(paths, app, store.preferences.keep_backups)?;
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": operation_id,
                "time": Utc::now(),
                "operation": "restore-target",
                "app": app,
                "restore_from_backup_id": backup_id,
                "backup_id": rollback_id,
                "warnings": process_warnings,
                "ok": true
            }),
        )?;
    }
    remove_pending(paths, app)?;
    println!("restored {app} from backup {backup_id}");
    Ok(())
}

fn resolve_target_paths(
    paths: &Paths,
    kind: &crate::app_definitions::KindDefinition,
    profile: &Profile,
) -> Result<Vec<(Option<crate::app_definitions::TargetDefinition>, PathBuf)>> {
    if profile.kind == "oauth_capture" {
        return Ok(oauth_target_paths(paths, kind, profile)?
            .into_iter()
            .map(|path| (None, path))
            .collect::<Vec<_>>());
    }
    kind.targets
        .iter()
        .map(|target| {
            Ok((
                Some(target.clone()),
                paths.expand_target_path(&target.path)?,
            ))
        })
        .collect::<Result<Vec<_>>>()
}

fn resolved_targets_from_paths(
    target_paths: &[(Option<crate::app_definitions::TargetDefinition>, PathBuf)],
) -> Vec<ResolvedTarget> {
    target_paths
        .iter()
        .map(|(_, path)| ResolvedTarget {
            target_id: format!("file:{}", path.display()),
            resolved_path: path.display().to_string(),
        })
        .collect()
}

struct ApplyUseContext<'a> {
    operation_id: &'a str,
    previous_id: Option<String>,
    backup_id: &'a str,
    resolved_targets: &'a [ResolvedTarget],
}

fn apply_and_verify_use(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
    target_paths: &[(Option<crate::app_definitions::TargetDefinition>, PathBuf)],
    context: ApplyUseContext<'_>,
) -> Result<Vec<Value>> {
    if profile.kind == "oauth_capture" {
        capture::apply_capture_to_live(paths, profile)?;
        apply_oauth_cleanup_targets(paths, definition, profile)?;
        write_pending(
            paths,
            &PendingSwitch {
                schema_version: 1,
                operation: "use".to_string(),
                operation_id: context.operation_id.to_string(),
                app: profile.app.clone(),
                from_profile: context.previous_id,
                to_profile: Some(profile.id.clone()),
                backup_id: Some(context.backup_id.to_string()),
                restore_from_backup_id: None,
                targets: context.resolved_targets.to_vec(),
                stage: "verifying".to_string(),
                expected: json!({ "kind": profile.kind, "profile": profile.id }),
            },
        )?;
        if !capture::live_matches_capture(paths, profile)? {
            return Err(anyhow!(
                "VerifyFailed: live capture targets do not match profile {}",
                profile.id
            ));
        }
        verify_oauth_cleanup_targets(paths, definition, profile)?;
        return verify_oauth_profile(paths, definition, profile);
    }
    for (target, path) in target_paths {
        handlers::apply_target(
            target.as_ref().expect("static target has a definition"),
            path,
            profile,
        )
        .with_context(|| format!("apply {}", path.display()))?;
    }
    for (target, path) in target_paths {
        if !handlers::static_status(
            target.as_ref().expect("static target has a definition"),
            path,
            profile,
        )? {
            return Err(anyhow!(
                "VerifyFailed: {} did not match profile {}",
                path.display(),
                profile.id
            ));
        }
    }
    Ok(Vec::new())
}

struct FailedUseRollback<'a> {
    profile: &'a Profile,
    previous_id: Option<String>,
    operation_id: &'a str,
    backup_id: &'a str,
    resolved_targets: &'a [ResolvedTarget],
    stage: &'a str,
    err: anyhow::Error,
}

fn rollback_failed_use(paths: &Paths, failed: FailedUseRollback<'_>) -> Result<()> {
    let message = failed.err.to_string();
    if let Err(rollback_err) = backup::restore_backup(paths, &failed.profile.app, failed.backup_id)
    {
        return Err(anyhow!("{message}; rollback failed: {rollback_err}"));
    }
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": failed.operation_id,
                "time": Utc::now(),
                "operation": "use",
                "app": failed.profile.app,
                "from_profile": failed.previous_id,
                "to_profile": failed.profile.id,
                "kind": failed.profile.kind,
                "backup_id": failed.backup_id,
                "recovered": true,
                "rolled_back": true,
                "stage": failed.stage,
                "targets": failed.resolved_targets,
                "error": message,
                "ok": false
            }),
        )?;
    }
    remove_pending(paths, &failed.profile.app)?;
    Err(anyhow!(message))
}

struct FailedRestoreRollback<'a> {
    app: &'a str,
    operation_id: &'a str,
    restore_from_backup_id: &'a str,
    rollback_backup_id: &'a str,
    resolved_targets: &'a [ResolvedTarget],
    stage: &'a str,
    err: anyhow::Error,
}

fn rollback_failed_restore(paths: &Paths, failed: FailedRestoreRollback<'_>) -> Result<()> {
    let message = failed.err.to_string();
    if let Err(rollback_err) = backup::restore_backup(paths, failed.app, failed.rollback_backup_id)
    {
        return Err(anyhow!("{message}; rollback failed: {rollback_err}"));
    }
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": failed.operation_id,
                "time": Utc::now(),
                "operation": "restore-target",
                "app": failed.app,
                "restore_from_backup_id": failed.restore_from_backup_id,
                "backup_id": failed.rollback_backup_id,
                "recovered": true,
                "rolled_back": true,
                "stage": failed.stage,
                "targets": failed.resolved_targets,
                "error": message,
                "ok": false
            }),
        )?;
    }
    remove_pending(paths, failed.app)?;
    Err(anyhow!(message))
}

fn recover_pending(
    paths: &Paths,
    registry: &DefinitionRegistry,
    store: &ProfileStore,
    pending: &PendingSwitch,
) -> Result<bool> {
    if matches!(pending.stage.as_str(), "applying" | "verifying") {
        return recover_pending_incomplete(paths, registry, store, pending);
    }
    if pending.stage != "bookkeeping" {
        return Ok(false);
    }
    match pending.operation.as_str() {
        "use" => {
            let to_profile = pending
                .to_profile
                .as_deref()
                .ok_or_else(|| anyhow!("InterruptedSwitch: pending use missing to_profile"))?;
            let profile = store
                .find(to_profile)
                .ok_or_else(|| anyhow!("ProfileNotFound: {to_profile}"))?;
            {
                let _state_lock = lock::acquire_state_lock(paths)?;
                let mut state = ActiveState::load(paths)?;
                state.active_profiles.insert(
                    pending.app.clone(),
                    Some(ActiveProfile {
                        id: to_profile.to_string(),
                        resolved_targets: pending.targets.clone(),
                    }),
                );
                state.save(paths)?;
                append_history(
                    paths,
                    &json!({
                        "operation_id": pending.operation_id,
                        "time": Utc::now(),
                        "operation": "use",
                        "app": pending.app,
                        "from_profile": pending.from_profile,
                        "to_profile": to_profile,
                        "kind": profile.kind,
                        "backup_id": pending.backup_id,
                        "recovered": true,
                        "ok": true
                    }),
                )?;
            }
            remove_pending(paths, &pending.app)?;
            Ok(true)
        }
        "restore-target" => {
            {
                let _state_lock = lock::acquire_state_lock(paths)?;
                append_history(
                    paths,
                    &json!({
                        "operation_id": pending.operation_id,
                        "time": Utc::now(),
                        "operation": "restore-target",
                        "app": pending.app,
                        "restore_from_backup_id": pending.restore_from_backup_id,
                        "backup_id": pending.backup_id,
                        "recovered": true,
                        "ok": true
                    }),
                )?;
            }
            remove_pending(paths, &pending.app)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn recover_pending_incomplete(
    paths: &Paths,
    registry: &DefinitionRegistry,
    store: &ProfileStore,
    pending: &PendingSwitch,
) -> Result<bool> {
    if pending.operation == "use" {
        if let Some(to_profile) = pending.to_profile.as_deref() {
            if let Some(profile) = store.find(to_profile) {
                let _target_locks = lock::acquire_target_locks(
                    paths,
                    pending
                        .targets
                        .iter()
                        .map(|target| target.target_id.clone())
                        .collect(),
                )?;
                if live_matches_profile(paths, registry, profile, &pending.targets)? {
                    recover_pending_use_bookkeeping(paths, store, pending)?;
                    return Ok(true);
                }
            }
        }
    }
    if pending.operation == "restore-target" {
        if let Some(restore_from_backup_id) = pending.restore_from_backup_id.as_deref() {
            let _target_locks = lock::acquire_target_locks(
                paths,
                pending
                    .targets
                    .iter()
                    .map(|target| target.target_id.clone())
                    .collect(),
            )?;
            if backup::live_matches_backup(paths, &pending.app, restore_from_backup_id)? {
                let _state_lock = lock::acquire_state_lock(paths)?;
                append_history(
                    paths,
                    &json!({
                        "operation_id": pending.operation_id,
                        "time": Utc::now(),
                        "operation": "restore-target",
                        "app": pending.app,
                        "restore_from_backup_id": pending.restore_from_backup_id,
                        "backup_id": pending.backup_id,
                        "recovered": true,
                        "completed_after_apply": true,
                        "stage": pending.stage,
                        "ok": true
                    }),
                )?;
                remove_pending(paths, &pending.app)?;
                return Ok(true);
            }
        }
    }
    recover_pending_by_rollback(paths, pending)
}

fn recover_pending_by_rollback(paths: &Paths, pending: &PendingSwitch) -> Result<bool> {
    let Some(backup_id) = pending.backup_id.as_deref() else {
        return Ok(false);
    };
    let _target_locks = lock::acquire_target_locks(
        paths,
        pending
            .targets
            .iter()
            .map(|target| target.target_id.clone())
            .collect(),
    )?;
    backup::restore_backup(paths, &pending.app, backup_id)
        .with_context(|| format!("InterruptedSwitch: rollback backup {backup_id}"))?;
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": pending.operation_id,
                "time": Utc::now(),
                "operation": pending.operation,
                "app": pending.app,
                "from_profile": pending.from_profile,
                "to_profile": pending.to_profile,
                "backup_id": pending.backup_id,
                "restore_from_backup_id": pending.restore_from_backup_id,
                "recovered": true,
                "rolled_back": true,
                "stage": pending.stage,
                "ok": false
            }),
        )?;
    }
    remove_pending(paths, &pending.app)?;
    Ok(true)
}

fn recover_pending_use_bookkeeping(
    paths: &Paths,
    store: &ProfileStore,
    pending: &PendingSwitch,
) -> Result<()> {
    let to_profile = pending
        .to_profile
        .as_deref()
        .ok_or_else(|| anyhow!("InterruptedSwitch: pending use missing to_profile"))?;
    let profile = store
        .find(to_profile)
        .ok_or_else(|| anyhow!("ProfileNotFound: {to_profile}"))?;
    let _state_lock = lock::acquire_state_lock(paths)?;
    let mut state = ActiveState::load(paths)?;
    state.active_profiles.insert(
        pending.app.clone(),
        Some(ActiveProfile {
            id: to_profile.to_string(),
            resolved_targets: pending.targets.clone(),
        }),
    );
    state.save(paths)?;
    append_history(
        paths,
        &json!({
            "operation_id": pending.operation_id,
            "time": Utc::now(),
            "operation": "use",
            "app": pending.app,
            "from_profile": pending.from_profile,
            "to_profile": to_profile,
            "kind": profile.kind,
            "backup_id": pending.backup_id,
            "recovered": true,
            "completed_after_apply": true,
            "stage": pending.stage,
            "ok": true
        }),
    )?;
    remove_pending(paths, &pending.app)
}

fn live_matches_profile(
    paths: &Paths,
    registry: &DefinitionRegistry,
    profile: &Profile,
    pending_targets: &[ResolvedTarget],
) -> Result<bool> {
    if profile.kind == "oauth_capture" {
        let loaded = registry.get(&profile.app)?;
        return Ok(capture::live_matches_capture(paths, profile)?
            && verify_oauth_profile(paths, &loaded.definition, profile).is_ok());
    }
    let loaded = registry.get(&profile.app)?;
    let kind = loaded
        .definition
        .kinds
        .get(&profile.kind)
        .ok_or_else(|| anyhow!("KindNotSupported: {}", profile.kind))?;
    if pending_targets.len() != kind.targets.len() {
        return Ok(false);
    }
    for (target, resolved) in kind.targets.iter().zip(pending_targets.iter()) {
        let path = PathBuf::from(&resolved.resolved_path);
        if !handlers::static_status(target, &path, profile)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn enforce_process_rule(
    definition: &crate::app_definitions::AppDefinition,
    oauth_io_required: bool,
    allow_running: bool,
    assume_app_stopped: bool,
    yes: bool,
) -> Result<Vec<Value>> {
    let running = process::detect_running(definition)?;
    if running.is_empty() {
        return Ok(Vec::new());
    }
    if oauth_io_required && assume_app_stopped && yes {
        return Ok(vec![json!({
            "type": "assume_app_stopped",
            "processes": running.iter().map(|process| json!({
                "pid": process.pid,
                "start_time": process.start_time,
                "command": process.command
            })).collect::<Vec<_>>()
        })]);
    }
    if oauth_io_required && assume_app_stopped && !yes {
        return Err(anyhow!(
            "--assume-app-stopped requires --yes for OAuth capture operations"
        ));
    }
    if oauth_io_required || !allow_running {
        return Err(anyhow!(process::format_app_running(
            &definition.app.id,
            &running
        )));
    }
    Ok(Vec::new())
}

fn combined_warnings(
    mut process_warnings: Vec<Value>,
    resolved_change_warning: Option<Value>,
    stale_warning: Option<Value>,
    identity_warnings: Vec<Value>,
) -> Vec<Value> {
    process_warnings.extend(resolved_change_warning);
    process_warnings.extend(stale_warning);
    process_warnings.extend(identity_warnings);
    process_warnings
}

fn oauth_stale_warning(paths: &Paths, profile: &Profile, stale_days: u64) -> Result<Option<Value>> {
    if profile.kind != "oauth_capture" {
        return Ok(None);
    }
    let manifest = capture::load_manifest(paths, &profile.id)?;
    let timestamp = manifest
        .last_writeback_at
        .as_deref()
        .unwrap_or(&manifest.captured_at);
    let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
        return Ok(Some(json!({
            "type": "oauth_capture_stale",
            "profile": profile.id,
            "reason": "capture manifest timestamp is invalid",
            "timestamp": timestamp
        })));
    };
    let age_days = Utc::now()
        .signed_duration_since(parsed.with_timezone(&Utc))
        .num_days();
    if age_days > stale_days as i64 {
        return Ok(Some(json!({
            "type": "oauth_capture_stale",
            "profile": profile.id,
            "age_days": age_days,
            "threshold_days": stale_days,
            "timestamp": timestamp,
            "hint": format!("run `any-switch import-current {} <name>` after confirming the target app can still refresh this account", profile.app)
        })));
    }
    Ok(None)
}

fn backup_inputs_for_profile(
    paths: &Paths,
    kind: &KindDefinition,
    profile: &Profile,
    target_paths: &[(Option<crate::app_definitions::TargetDefinition>, PathBuf)],
    oauth_io_required: bool,
) -> Result<Vec<backup::BackupInput>> {
    if profile.kind == "oauth_capture" {
        return oauth_backup_inputs(paths, kind, profile);
    }
    Ok(target_paths
        .iter()
        .map(|(_, path)| backup::BackupInput::file(path.clone(), oauth_io_required))
        .collect())
}

fn backup_input_plan(input: &backup::BackupInput) -> Value {
    let target_id = input.target_id();
    let requires_app_stopped = input.requires_app_stopped();
    match input {
        backup::BackupInput::File { path, .. } => json!({
            "target_id": target_id,
            "type": "file",
            "path": path.display().to_string(),
            "requires_app_stopped": requires_app_stopped,
        }),
        backup::BackupInput::JsonSubtree {
            path, json_path, ..
        } => json!({
            "target_id": target_id,
            "type": "json_subtree",
            "path": path.display().to_string(),
            "json_path": json_path,
            "requires_app_stopped": requires_app_stopped,
        }),
        backup::BackupInput::TomlManagedPaths {
            path, toml_paths, ..
        } => json!({
            "target_id": target_id,
            "type": "toml_managed_paths",
            "path": path.display().to_string(),
            "toml_paths": toml_paths,
            "requires_app_stopped": requires_app_stopped,
        }),
        backup::BackupInput::SecretEntry {
            backend,
            service,
            account,
            resolved_account,
            ..
        } => json!({
            "target_id": target_id,
            "type": "secret_entry",
            "backend": backend,
            "service": service,
            "account": account,
            "resolved_account": resolved_account,
            "requires_app_stopped": requires_app_stopped,
        }),
    }
}

fn post_write_verify_plan(
    definition: &AppDefinition,
    profile: &Profile,
    target_paths: &[(Option<crate::app_definitions::TargetDefinition>, PathBuf)],
) -> Result<Value> {
    if profile.kind == "oauth_capture" {
        let mut required_identity = serde_json::Map::new();
        for key in required_identity_keys(definition, &profile.kind)? {
            required_identity.insert(
                key.clone(),
                profile.identity.get(&key).cloned().unwrap_or(Value::Null),
            );
        }
        return Ok(json!({
            "type": "oauth_identity",
            "required_identity": required_identity,
        }));
    }

    Ok(json!({
        "type": "static_targets",
        "targets": target_paths
            .iter()
            .map(|(target, path)| {
                json!({
                    "target_id": format!("file:{}", path.display()),
                    "handler": target.as_ref().map(|target| target.handler.as_str()),
                    "path": path.display().to_string(),
                })
            })
            .collect::<Vec<_>>(),
    }))
}

fn oauth_backup_inputs(
    paths: &Paths,
    kind: &KindDefinition,
    profile: &Profile,
) -> Result<Vec<backup::BackupInput>> {
    let spec = CaptureSpec::from_profile(profile)?;
    let mut out = Vec::new();
    for source in spec
        .sources
        .iter()
        .filter(|source| source.applies_to_current_platform())
    {
        match source.source_type.as_str() {
            "file" => out.push(backup::BackupInput::File {
                path: capture::source_path(paths, source)?,
                requires_app_stopped: true,
            }),
            "json_subtree" => out.push(backup::BackupInput::JsonSubtree {
                path: capture::source_path(paths, source)?,
                json_path: source
                    .json_path
                    .clone()
                    .ok_or_else(|| anyhow!("json_subtree source missing json_path"))?,
                requires_app_stopped: true,
            }),
            "toml_managed_paths" => out.push(backup::BackupInput::TomlManagedPaths {
                path: capture::source_path(paths, source)?,
                toml_paths: source.toml_paths.clone(),
                requires_app_stopped: true,
            }),
            "secret_entry" => {
                let service = source.service.clone().ok_or_else(|| {
                    anyhow!("secret_entry source {} missing service", source.stored_as)
                })?;
                let account = source.account.clone().ok_or_else(|| {
                    anyhow!("secret_entry source {} missing account", source.stored_as)
                })?;
                let resolved_account =
                    account.replace("${MACOS_USER}", &crate::paths::current_os_user());
                out.push(backup::BackupInput::SecretEntry {
                    backend: source
                        .backend
                        .clone()
                        .unwrap_or_else(|| "macos_keychain".to_string()),
                    service,
                    account,
                    resolved_account,
                    requires_app_stopped: true,
                });
            }
            other => {
                return Err(anyhow!(
                    "UnknownHandler: unsupported capture source {other}"
                ))
            }
        }
    }
    for cleanup in &kind.cleanup_targets {
        out.push(backup::BackupInput::File {
            path: paths.expand_target_path(&cleanup.path)?,
            requires_app_stopped: cleanup.requires_app_stopped,
        });
    }
    Ok(out)
}

fn oauth_target_paths(
    paths: &Paths,
    kind: &KindDefinition,
    profile: &Profile,
) -> Result<Vec<PathBuf>> {
    let spec = CaptureSpec::from_profile(profile)?;
    let mut out = Vec::new();
    for source in spec
        .sources
        .iter()
        .filter(|source| source.applies_to_current_platform())
    {
        if matches!(
            source.source_type.as_str(),
            "file" | "json_subtree" | "toml_managed_paths"
        ) {
            let path = capture::source_path(paths, source)?;
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    for cleanup in &kind.cleanup_targets {
        let path = paths.expand_target_path(&cleanup.path)?;
        if !out.contains(&path) {
            out.push(path);
        }
    }
    Ok(out)
}

fn apply_oauth_cleanup_targets(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
) -> Result<()> {
    let Some(kind) = definition.kinds.get(&profile.kind) else {
        return Ok(());
    };
    for cleanup in &kind.cleanup_targets {
        apply_oauth_cleanup_target(paths, cleanup)?;
    }
    Ok(())
}

fn apply_oauth_cleanup_target(paths: &Paths, cleanup: &CleanupTargetDefinition) -> Result<()> {
    match cleanup.handler.as_str() {
        "json_remove_keys" => {
            let keys = cleanup.keys.iter().map(String::as_str).collect::<Vec<_>>();
            handlers::remove_json_object_keys(
                &paths.expand_target_path(&cleanup.path)?,
                &cleanup.json_path,
                &keys,
            )
        }
        other => Err(anyhow!("UnknownHandler: {other}")),
    }
}

fn verify_oauth_cleanup_targets(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
) -> Result<()> {
    let Some(kind) = definition.kinds.get(&profile.kind) else {
        return Ok(());
    };
    for cleanup in &kind.cleanup_targets {
        verify_oauth_cleanup_target(paths, cleanup)?;
    }
    Ok(())
}

fn verify_oauth_cleanup_target(paths: &Paths, cleanup: &CleanupTargetDefinition) -> Result<()> {
    match cleanup.handler.as_str() {
        "json_remove_keys" => {
            let keys = cleanup.keys.iter().map(String::as_str).collect::<Vec<_>>();
            if handlers::json_object_has_any_key(
                &paths.expand_target_path(&cleanup.path)?,
                &cleanup.json_path,
                &keys,
            )? {
                return Err(anyhow!(
                    "VerifyFailed: cleanup target still contains managed keys"
                ));
            }
            Ok(())
        }
        other => Err(anyhow!("UnknownHandler: {other}")),
    }
}

fn writeback_oauth_profile(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
) -> Result<()> {
    let live_identity = read_live_identity(paths, definition, &profile.kind)?;
    if !required_identity_matches_definition(
        definition,
        &profile.kind,
        &profile.identity,
        &live_identity,
    )? {
        return Err(anyhow!(
            "DriftBeforeWriteback: live identity no longer matches active profile {}",
            profile.id
        ));
    }
    ensure_oauth_source_consistency(
        paths,
        definition,
        profile,
        &live_identity,
        "DriftBeforeWriteback",
    )?;
    let spec = CaptureSpec::from_profile(profile)?;
    let files = capture::capture_files_from_live(paths, &spec)?;
    capture::write_capture_files(paths, &profile.id, files, true)?;
    Ok(())
}

fn verify_oauth_profile(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
) -> Result<Vec<Value>> {
    let live_identity = read_live_identity(paths, definition, &profile.kind)?;
    if !required_identity_matches_definition(
        definition,
        &profile.kind,
        &profile.identity,
        &live_identity,
    )? {
        return Err(anyhow!(
            "IdentityMismatch: restored identity does not match profile {}",
            profile.id
        ));
    }
    ensure_oauth_source_consistency(
        paths,
        definition,
        profile,
        &live_identity,
        "IdentityMismatch",
    )?;
    Ok(optional_identity_warnings(
        definition,
        profile,
        &live_identity,
    ))
}

fn ensure_oauth_source_consistency(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
    live_identity: &IndexMap<String, Value>,
    error_kind: &str,
) -> Result<()> {
    let Some(kind) = definition.kinds.get(&profile.kind) else {
        return Ok(());
    };
    for source in &kind.capture_sources {
        let Some(source_identity) = read_definition_source_identity(paths, source)? else {
            continue;
        };
        ensure_source_identity_matches_definition(
            definition,
            &profile.kind,
            live_identity,
            &source_identity,
            error_kind,
        )?;
        ensure_source_identity_matches_definition(
            definition,
            &profile.kind,
            &profile.identity,
            &source_identity,
            error_kind,
        )?;
    }
    Ok(())
}

fn read_definition_source_identity(
    paths: &Paths,
    source: &CaptureSourceDefinition,
) -> Result<Option<IndexMap<String, Value>>> {
    let Some(identity_definition) = &source.identity else {
        return Ok(None);
    };
    let bytes = match source.handler.as_str() {
        "file_capture" => {
            let path = source
                .path
                .as_deref()
                .ok_or_else(|| anyhow!("file_capture source {} missing path", source.stored_as))?;
            let path = paths.expand_target_path(path)?;
            if !path.exists() {
                return Ok(None);
            }
            fs::read(path)?
        }
        "secret_entry" => {
            let service = source.service.as_deref().ok_or_else(|| {
                anyhow!("secret_entry source {} missing service", source.stored_as)
            })?;
            let account = source.account.as_deref().ok_or_else(|| {
                anyhow!("secret_entry source {} missing account", source.stored_as)
            })?;
            let resolved_account =
                account.replace("${MACOS_USER}", &crate::paths::current_os_user());
            let Ok(bytes) = crate::keychain::read_generic_password(service, &resolved_account)
            else {
                return Ok(None);
            };
            bytes
        }
        other => {
            return Err(anyhow!(
                "UnknownHandler: unsupported capture source {other}"
            ))
        }
    };
    let root: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("read source identity from {}", source.stored_as))?;
    let identity = identity::extract_identity_from_definition(&root, identity_definition)?;
    Ok((!identity.is_empty()).then_some(identity))
}

fn ensure_source_identity_matches_definition(
    definition: &AppDefinition,
    kind: &str,
    expected: &IndexMap<String, Value>,
    actual: &IndexMap<String, Value>,
    error_kind: &str,
) -> Result<()> {
    if !required_identity_matches_definition(definition, kind, expected, actual)? {
        return Err(anyhow!(
            "{error_kind}: credential source identity does not match target/profile identity"
        ));
    }
    Ok(())
}

fn optional_identity_warnings(
    definition: &AppDefinition,
    profile: &Profile,
    live_identity: &IndexMap<String, Value>,
) -> Vec<Value> {
    let required_keys = required_identity_keys(definition, &profile.kind).unwrap_or_default();
    profile
        .identity
        .iter()
        .filter(|(key, expected)| {
            !required_keys.iter().any(|required| required == *key)
                && live_identity
                    .get(*key)
                    .is_some_and(|actual| actual != *expected)
        })
        .map(|(key, expected)| {
            json!({
                "type": "optional_identity_mismatch",
                "field": key,
                "expected": expected,
                "actual": live_identity.get(key)
            })
        })
        .collect()
}

fn required_identity_matches_definition(
    definition: &AppDefinition,
    kind: &str,
    expected: &IndexMap<String, Value>,
    actual: &IndexMap<String, Value>,
) -> Result<bool> {
    let required_keys = required_identity_keys(definition, kind)?;
    Ok(required_keys
        .iter()
        .all(|key| expected.get(key).is_some() && expected.get(key) == actual.get(key)))
}

fn required_identity_keys(definition: &AppDefinition, kind: &str) -> Result<Vec<String>> {
    let kind = definition
        .kinds
        .get(kind)
        .ok_or_else(|| anyhow!("KindNotSupported: {kind}"))?;
    Ok(kind
        .identity
        .as_ref()
        .map(|identity| {
            identity
                .fields
                .iter()
                .filter(|(_, field)| field.verify == "required")
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default())
}

fn read_live_identity(
    paths: &Paths,
    definition: &AppDefinition,
    kind_name: &str,
) -> Result<IndexMap<String, Value>> {
    let kind = definition
        .kinds
        .get(kind_name)
        .ok_or_else(|| anyhow!("KindNotSupported: {kind_name}"))?;
    let identity_definition = kind
        .identity
        .as_ref()
        .ok_or_else(|| anyhow!("IdentityMissing: {kind_name} has no identity definition"))?;
    let mut root = Value::Object(Default::default());
    let mut read_any = false;
    for target in &kind.targets {
        if !matches!(target.handler.as_str(), "file_capture" | "json_subtree") {
            continue;
        }
        let path = paths.expand_target_path(&target.path)?;
        if !path.exists() {
            continue;
        }
        let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
        merge_identity_root(&mut root, value);
        read_any = true;
    }
    if !read_any {
        return Err(anyhow!(
            "TargetMissing: no identity source targets for {} {}",
            definition.app.id,
            kind_name
        ));
    }
    identity::extract_identity_from_definition(&root, identity_definition)
}

fn merge_identity_root(root: &mut Value, value: Value) {
    match (root, value) {
        (Value::Object(base), Value::Object(incoming)) => {
            for (key, value) in incoming {
                base.entry(key).or_insert(value);
            }
        }
        (root, value) if root.as_object().is_some_and(|object| object.is_empty()) => {
            *root = value;
        }
        _ => {}
    }
}

fn status_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    app: Option<&str>,
    as_json: bool,
) -> Result<()> {
    if let Some(app) = app {
        registry.get(app)?;
    }
    let store = ProfileStore::load(paths)?;
    let state = ActiveState::load(paths)?;
    let mut rows = Vec::new();
    for (app_id, loaded) in registry
        .iter()
        .filter(|(app_id, _)| app.is_none_or(|app| app == app_id.as_str()))
    {
        if let Some(pending) = load_pending(paths, app_id)? {
            rows.push(json!({
                "app": app_id,
                "status": "interrupted",
                "operation": pending.operation,
                "operation_id": pending.operation_id,
                "stage": pending.stage
            }));
            continue;
        }
        let active = state.active_profiles.get(app_id).and_then(|v| v.as_ref());
        let Some(active) = active else {
            rows.push(
                json!({"app": app_id, "status": "no-active", "hint": no_active_hint(app_id)}),
            );
            continue;
        };
        let Some(profile) = store.find(&active.id) else {
            rows.push(json!({"app": app_id, "status": "missing", "active": active.id}));
            continue;
        };
        let Some(kind) = loaded.definition.kinds.get(&profile.kind) else {
            rows.push(json!({"app": app_id, "status": "missing", "active": active.id}));
            continue;
        };
        let current_resolved_targets = match resolve_target_paths(paths, kind, profile) {
            Ok(target_paths) => resolved_targets_from_paths(&target_paths),
            Err(err) => {
                rows.push(json!({
                    "app": app_id,
                    "active": active.id,
                    "status": "drifted",
                    "reason": err.to_string()
                }));
                continue;
            }
        };
        if !active.resolved_targets.is_empty()
            && active.resolved_targets != current_resolved_targets
        {
            rows.push(json!({
                "app": app_id,
                "active": active.id,
                "status": "drifted",
                "reason": "resolved_targets_changed",
                "old_resolved_targets": active.resolved_targets.clone(),
                "new_resolved_targets": current_resolved_targets
            }));
            continue;
        }
        let (matched, missing, reason) = if profile.kind == "oauth_capture" {
            match capture::ensure_capture_complete(paths, profile)
                .and_then(|_| verify_oauth_profile(paths, &loaded.definition, profile).map(|_| ()))
            {
                Ok(()) => (true, false, None),
                Err(err) => (false, false, Some(err.to_string())),
            }
        } else {
            let mut matched = true;
            let mut missing = false;
            for target in &kind.targets {
                let path = paths.expand_target_path(&target.path)?;
                if !path.exists() {
                    matched = false;
                    missing = true;
                    continue;
                }
                if !handlers::static_status(target, &path, profile)? {
                    matched = false;
                }
            }
            let reason = missing.then_some("target_missing".to_string());
            (matched, missing, reason)
        };
        let override_reasons = if matched {
            status_override_reasons(paths, &loaded.definition, profile)?
        } else {
            Vec::new()
        };
        let status = if missing {
            "missing"
        } else if !matched {
            "drifted"
        } else if override_reasons.is_empty() {
            "matched"
        } else {
            "matched-with-overrides"
        };
        rows.push(json!({
            "app": app_id,
            "active": active.id,
            "status": status,
            "reason": reason,
            "override_reasons": override_reasons
        }));
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for row in rows {
            println!(
                "{}\t{}\t{}",
                row["app"].as_str().unwrap_or_default(),
                row["status"].as_str().unwrap_or_default(),
                row.get("active").and_then(Value::as_str).unwrap_or("")
            );
            if let Some(hint) = row.get("hint").and_then(Value::as_str) {
                println!("{hint}");
            }
            if let Some(reason) = row.get("reason").and_then(Value::as_str) {
                println!("reason\t{reason}");
            }
            if let Some(old_targets) = row.get("old_resolved_targets").and_then(Value::as_array) {
                for target in old_targets {
                    println!("old_target\t{}", serde_json::to_string(target)?);
                }
            }
            if let Some(new_targets) = row.get("new_resolved_targets").and_then(Value::as_array) {
                for target in new_targets {
                    println!("new_target\t{}", serde_json::to_string(target)?);
                }
            }
            if let Some(reasons) = row.get("override_reasons").and_then(Value::as_array) {
                for reason in reasons.iter().filter_map(Value::as_str) {
                    println!("override\t{reason}");
                }
            }
        }
    }
    Ok(())
}

fn status_override_reasons(
    paths: &Paths,
    definition: &AppDefinition,
    profile: &Profile,
) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    for check in &definition.override_checks {
        if !check.applies_to_kinds.is_empty()
            && !check
                .applies_to_kinds
                .iter()
                .any(|kind| kind == &profile.kind)
        {
            continue;
        }
        collect_override_check_reasons(paths, check, &mut reasons)?;
    }
    Ok(reasons)
}

fn collect_override_check_reasons(
    paths: &Paths,
    check: &crate::app_definitions::OverrideCheckDefinition,
    reasons: &mut Vec<String>,
) -> Result<()> {
    match check.handler.as_str() {
        "process_env_non_empty" => {
            for name in &check.env_names {
                if env::var(name).is_ok_and(|value| !value.is_empty()) {
                    reasons.push(format!("process_env:{name}"));
                }
            }
        }
        "json_object_keys_non_empty" => {
            let path = paths.expand_target_path(check.path.as_deref().unwrap_or_default())?;
            if path.exists() {
                if let Some(object) =
                    handlers::read_json_path(&path, check.json_path.as_deref().unwrap_or("$"))?
                {
                    for key in &check.keys {
                        if object
                            .get(key)
                            .and_then(Value::as_str)
                            .is_some_and(|value| !value.is_empty())
                        {
                            reasons.push(format!(
                                "{}:{key}",
                                check.reason_prefix.as_deref().unwrap_or("json")
                            ));
                        }
                    }
                }
            }
        }
        "json_string_non_empty" => {
            let path = paths.expand_target_path(check.path.as_deref().unwrap_or_default())?;
            if path.exists()
                && handlers::read_json_path(&path, check.json_path.as_deref().unwrap_or("$"))?
                    .and_then(|value| value.as_str().map(str::to_string))
                    .is_some_and(|value| !value.is_empty())
            {
                reasons.push(check.reason.clone().unwrap_or_else(|| "json".to_string()));
            }
        }
        "managed_json_object_keys_non_empty" | "managed_json_path_present" => {
            collect_managed_json_override_reasons(check, reasons)?;
        }
        other => return Err(anyhow!("UnknownHandler: {other}")),
    }
    Ok(())
}

fn managed_json_dirs(check: &crate::app_definitions::OverrideCheckDefinition) -> Vec<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(raw) = check.test_env.as_deref().and_then(env::var_os) {
        return vec![PathBuf::from(raw)];
    }

    #[cfg(target_os = "macos")]
    {
        check
            .macos_dir
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        check
            .linux_dir
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    }
    #[cfg(windows)]
    {
        check
            .windows_dir
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    }
    #[cfg(not(any(unix, windows)))]
    {
        Vec::new()
    }
}

fn collect_managed_json_override_reasons(
    check: &crate::app_definitions::OverrideCheckDefinition,
    reasons: &mut Vec<String>,
) -> Result<()> {
    for dir in managed_json_dirs(check) {
        let merged = read_managed_json_settings(&dir)?;
        let prefix = format!("managed_settings:{}", dir.display());
        match check.handler.as_str() {
            "managed_json_object_keys_non_empty" => {
                if let Some(object) =
                    value_at_simple_json_path(&merged, check.json_path.as_deref().unwrap_or("$"))?
                {
                    for key in &check.keys {
                        if object
                            .get(key)
                            .and_then(Value::as_str)
                            .is_some_and(|value| !value.is_empty())
                        {
                            reasons.push(format!(
                                "{prefix}:{}:{key}",
                                check.reason_prefix.as_deref().unwrap_or("json")
                            ));
                        }
                    }
                }
            }
            "managed_json_path_present" => {
                if value_at_simple_json_path(&merged, check.json_path.as_deref().unwrap_or("$"))?
                    .is_some()
                {
                    reasons.push(format!(
                        "{prefix}:{}",
                        check.reason.as_deref().unwrap_or("json")
                    ));
                }
            }
            other => return Err(anyhow!("UnknownHandler: {other}")),
        }
    }
    Ok(())
}

fn read_managed_json_settings(dir: &Path) -> Result<Value> {
    let mut merged = Value::Object(serde_json::Map::new());
    let base = dir.join("managed-settings.json");
    if base.exists() {
        merge_json_objects(&mut merged, serde_json::from_slice(&fs::read(&base)?)?);
    }
    let drop_in_dir = dir.join("managed-settings.d");
    if drop_in_dir.exists() {
        let mut files = fs::read_dir(&drop_in_dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
                    && path.extension().and_then(|extension| extension.to_str()) == Some("json")
            })
            .collect::<Vec<_>>();
        files.sort();
        for path in files {
            merge_json_objects(&mut merged, serde_json::from_slice(&fs::read(path)?)?);
        }
    }
    Ok(merged)
}

fn merge_json_objects(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                if let Some(base_value) = base.get_mut(&key) {
                    merge_json_objects(base_value, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn remove_command(paths: &Paths, id: &str, yes: bool) -> Result<()> {
    if !yes {
        return Err(anyhow!("remove requires --yes or --force in this build"));
    }
    let _profiles_lock = FileLock::acquire(lock::profiles_lock(paths))?;
    let mut store = ProfileStore::load(paths)?;
    let profile = store
        .remove(id)
        .ok_or_else(|| anyhow!("ProfileNotFound: {id}"))?;
    let _app_lock = FileLock::acquire(lock::app_lock(paths, &profile.app)?)?;
    if let Some(pending) = load_pending(paths, &profile.app)? {
        return Err(anyhow!(
            "InterruptedSwitch: {} has pending {} operation {} at stage {}",
            profile.app,
            pending.operation,
            pending.operation_id,
            pending.stage
        ));
    }
    store.save(paths)?;
    let capture_dir = paths.switch_home.join("captures").join(id);
    if capture_dir.exists() {
        fs::remove_dir_all(capture_dir)?;
    }
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        let mut state = ActiveState::load(paths)?;
        if state
            .active_profiles
            .get(&profile.app)
            .and_then(|v| v.as_ref())
            .is_some_and(|active| active.id == id)
        {
            state.active_profiles.insert(profile.app.clone(), None);
            state.save(paths)?;
        }
        append_history(
            paths,
            &json!({
                "operation_id": uuid::Uuid::now_v7().to_string(),
                "time": Utc::now(),
                "operation": "remove",
                "app": profile.app,
                "profile": id,
                "ok": true
            }),
        )?;
    }
    println!("removed {id}");
    Ok(())
}

fn detach_command(paths: &Paths, registry: &DefinitionRegistry, app: &str) -> Result<()> {
    let app_lock_path = lock::app_lock(paths, app)?;
    registry.get(app)?;
    let _app_lock = FileLock::acquire(app_lock_path)?;
    if let Some(pending) = load_pending(paths, app)? {
        return Err(anyhow!(
            "InterruptedSwitch: {app} has pending {} operation {} at stage {}",
            pending.operation,
            pending.operation_id,
            pending.stage
        ));
    }
    {
        let _state_lock = lock::acquire_state_lock(paths)?;
        let mut state = ActiveState::load(paths)?;
        let previous = state.active_profiles.insert(app.to_string(), None);
        state.save(paths)?;
        append_history(
            paths,
            &json!({
                "operation_id": uuid::Uuid::now_v7().to_string(),
                "time": Utc::now(),
                "operation": "detach",
                "app": app,
                "from_profile": previous.and_then(|v| v.map(|p| p.id)),
                "ok": true
            }),
        )?;
    }
    println!("{app} is now detached.");
    println!("{}", no_active_hint(app));
    Ok(())
}

fn doctor_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    app: Option<&str>,
    startup_permission_warnings: Option<Vec<String>>,
    json_output: bool,
) -> Result<()> {
    if json_output {
        return doctor_json_command(paths, registry, app, startup_permission_warnings);
    }
    println!("any_switch_home\t{}", paths.switch_home.display());
    println!("profiles_yaml\t{}", paths.profiles_path().display());
    println!(
        "profiles.yaml secret-leak surface\t{}",
        cloud_sync_warning(paths)
    );
    let permission_warnings = match startup_permission_warnings {
        Some(warnings) => warnings,
        None => permission_warnings(paths)?,
    };
    if permission_warnings.is_empty() {
        println!("permissions\tok");
    } else {
        for warning in permission_warnings {
            println!("permissions\twarning: {warning}");
        }
    }
    for usage in backup::backup_usage(paths, app)? {
        let warning = if usage.logical_bytes > 100 * 1024 * 1024 {
            "\twarning: exceeds 100 MB soft limit"
        } else {
            ""
        };
        println!(
            "backups\t{}\tcount={}\tinode_bytes={}\tlogical_bytes={}{}",
            usage.app, usage.backup_count, usage.inode_bytes, usage.logical_bytes, warning
        );
    }
    if let Some(app) = app {
        println!("app\t{app}");
        let loaded = registry.get(app)?;
        doctor_definition(paths, &loaded.definition)?;
        match process::detect_running(&loaded.definition) {
            Ok(running) if running.is_empty() => println!("processes\tok"),
            Ok(running) => {
                for process in running {
                    println!(
                        "process\tpid={}\tstart_time={}\tcommand={}",
                        process.pid,
                        process.start_time.as_deref().unwrap_or("unknown"),
                        process.command
                    );
                }
            }
            Err(err) => println!("processes\twarning: {err}"),
        }
        let store = ProfileStore::load(paths)?;
        let state = ActiveState::load(paths)?;
        if let Some(active) = state
            .active_profiles
            .get(app)
            .and_then(|entry| entry.as_ref())
        {
            println!("active_profile\t{}", active.id);
            if let Some(profile) = store.find(&active.id) {
                println!("active_kind\t{}", profile.kind);
                if !profile.identity.is_empty() {
                    println!("identity\t{}", serde_json::to_string(&profile.identity)?);
                }
                if profile.kind == "oauth_capture" {
                    match capture::ensure_capture_complete(paths, profile) {
                        Ok(()) => println!("capture\tok"),
                        Err(err) => println!("capture\twarning: {err}"),
                    }
                    match verify_oauth_profile(paths, &loaded.definition, profile) {
                        Ok(warnings) if warnings.is_empty() => println!("identity_check\tok"),
                        Ok(warnings) => {
                            for warning in warnings {
                                println!(
                                    "identity_check\twarning: {}",
                                    serde_json::to_string(&warning)?
                                );
                            }
                        }
                        Err(err) => println!("identity_check\twarning: {err}"),
                    }
                }
                let override_reasons = status_override_reasons(paths, &loaded.definition, profile)?;
                if override_reasons.is_empty() {
                    println!("overrides\tok");
                } else {
                    for reason in override_reasons {
                        println!("override\t{reason}");
                    }
                }
                if let Some(kind) = loaded.definition.kinds.get(&profile.kind) {
                    match resolve_target_paths(paths, kind, profile) {
                        Ok(targets) => {
                            for (_, path) in targets {
                                println!("target\t{}", path.display());
                            }
                        }
                        Err(err) => println!("target\twarning: {err}"),
                    }
                }
            } else {
                println!("active_profile_missing\t{}", active.id);
            }
        } else {
            println!("active_profile\tnone");
        }
    }
    Ok(())
}

fn doctor_json_command(
    paths: &Paths,
    registry: &DefinitionRegistry,
    app: Option<&str>,
    startup_permission_warnings: Option<Vec<String>>,
) -> Result<()> {
    let permission_warnings = match startup_permission_warnings {
        Some(warnings) => warnings,
        None => permission_warnings(paths)?,
    };
    let backup_usage = backup::backup_usage(paths, app)?
        .into_iter()
        .map(|usage| {
            json!({
                "app": usage.app,
                "count": usage.backup_count,
                "inode_bytes": usage.inode_bytes,
                "logical_bytes": usage.logical_bytes,
                "warning": (usage.logical_bytes > 100 * 1024 * 1024).then_some("exceeds 100 MB soft limit")
            })
        })
        .collect::<Vec<_>>();
    let mut output = json!({
        "any_switch_home": paths.switch_home,
        "profiles_yaml": paths.profiles_path(),
        "profiles_yaml_secret_leak_surface": cloud_sync_warning(paths),
        "permissions": {
            "status": if permission_warnings.is_empty() { "ok" } else { "warning" },
            "warnings": permission_warnings,
        },
        "backups": backup_usage,
        "app": Value::Null,
    });

    if let Some(app) = app {
        let loaded = registry.get(app)?;
        let (process_status, process_warning, processes) =
            match process::detect_running(&loaded.definition) {
                Ok(running) => {
                    let processes = running
                        .into_iter()
                        .map(|process| {
                            json!({
                                "pid": process.pid,
                                "start_time": process.start_time,
                                "command": process.command,
                            })
                        })
                        .collect::<Vec<_>>();
                    let status = if processes.is_empty() {
                        "ok"
                    } else {
                        "running"
                    };
                    (status, Value::Null, processes)
                }
                Err(err) => ("warning", json!(err.to_string()), Vec::new()),
            };
        let store = ProfileStore::load(paths)?;
        let state = ActiveState::load(paths)?;
        let active = doctor_json_active(paths, &loaded.definition, app, &store, &state)?;
        output["app"] = json!({
            "id": app,
            "definition": doctor_definition_records(paths, &loaded.definition)?,
            "processes": processes,
            "process_status": process_status,
            "process_warning": process_warning,
            "active": active,
        });
    }

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn doctor_json_active(
    paths: &Paths,
    definition: &AppDefinition,
    app: &str,
    store: &ProfileStore,
    state: &ActiveState,
) -> Result<Value> {
    let Some(active) = state
        .active_profiles
        .get(app)
        .and_then(|entry| entry.as_ref())
    else {
        return Ok(json!({ "status": "none" }));
    };
    let Some(profile) = store.find(&active.id) else {
        return Ok(json!({
            "status": "missing",
            "profile": active.id,
        }));
    };
    let capture = if profile.kind == "oauth_capture" {
        Some(match capture::ensure_capture_complete(paths, profile) {
            Ok(()) => json!({ "status": "ok" }),
            Err(err) => json!({ "status": "warning", "warning": err.to_string() }),
        })
    } else {
        None
    };
    let identity_check = if profile.kind == "oauth_capture" {
        Some(match verify_oauth_profile(paths, definition, profile) {
            Ok(warnings) if warnings.is_empty() => json!({ "status": "ok" }),
            Ok(warnings) => json!({ "status": "warning", "warnings": warnings }),
            Err(err) => json!({ "status": "warning", "warning": err.to_string() }),
        })
    } else {
        None
    };
    let override_reasons = status_override_reasons(paths, definition, profile)?;
    let targets = if let Some(kind) = definition.kinds.get(&profile.kind) {
        match resolve_target_paths(paths, kind, profile) {
            Ok(targets) => json!({
                "status": "ok",
                "paths": targets
                    .into_iter()
                    .map(|(_, path)| path.display().to_string())
                    .collect::<Vec<_>>(),
            }),
            Err(err) => json!({ "status": "warning", "warning": err.to_string() }),
        }
    } else {
        json!({ "status": "missing_kind" })
    };

    Ok(json!({
        "status": "active",
        "profile": profile.id,
        "kind": profile.kind,
        "identity": profile.identity,
        "capture": capture,
        "identity_check": identity_check,
        "override_status": if override_reasons.is_empty() { "ok" } else { "warning" },
        "override_reasons": override_reasons,
        "targets": targets,
    }))
}

fn doctor_definition_records(paths: &Paths, definition: &AppDefinition) -> Result<Vec<Value>> {
    let mut records = doctor_definition_json_field_records(paths, definition)?;
    records.extend(doctor_definition_json_object_schema_records(
        paths, definition,
    )?);
    for (kind_name, kind) in &definition.kinds {
        for target in &kind.targets {
            let path = match paths.expand_target_path(&target.path) {
                Ok(path) => path,
                Err(err) => {
                    records.push(json!({
                        "type": "target",
                        "kind": kind_name,
                        "handler": target.handler,
                        "status": "warning",
                        "warning": err.to_string(),
                    }));
                    continue;
                }
            };
            records.push(json!({
                "type": "target",
                "kind": kind_name,
                "handler": target.handler,
                "status": if path.exists() { "exists" } else { "missing" },
                "path": path,
            }));
            if target.handler == "json_env_merge" && path.exists() {
                let present = target_present_json_env_keys(&path, target)?;
                records.push(json!({
                    "type": "managed_keys",
                    "kind": kind_name,
                    "keys": present,
                }));
            }
            if let Some(json_path) = target.json_path.as_deref().filter(|_| path.exists()) {
                records.push(json!({
                    "type": "json_path",
                    "kind": kind_name,
                    "handler": target.handler,
                    "status": if handlers::read_json_path(&path, json_path)?.is_some() {
                        "present"
                    } else {
                        "missing"
                    },
                    "path": path,
                    "json_path": json_path,
                }));
            }
        }
        if kind.identity.is_some() {
            match read_live_identity(paths, definition, kind_name) {
                Ok(identity) if identity.is_empty() => records.push(json!({
                    "type": "identity",
                    "kind": kind_name,
                    "status": "missing",
                })),
                Ok(identity) => records.push(json!({
                    "type": "identity",
                    "kind": kind_name,
                    "status": "present",
                    "identity": identity,
                })),
                Err(err) => records.push(json!({
                    "type": "identity",
                    "kind": kind_name,
                    "status": "warning",
                    "warning": err.to_string(),
                })),
            }
        }
    }
    Ok(records)
}

fn doctor_definition_json_object_schema_records(
    paths: &Paths,
    definition: &AppDefinition,
) -> Result<Vec<Value>> {
    let mut records = Vec::new();
    for schema in &definition.doctor.json_object_schemas {
        let path = match paths.expand_target_path(&schema.path) {
            Ok(path) => path,
            Err(err) => {
                records.push(json!({
                    "type": "json_object_schema",
                    "name": schema.name,
                    "status": "warning",
                    "warning": err.to_string(),
                    "path_template": schema.path,
                    "json_path": schema.json_path,
                }));
                continue;
            }
        };
        if !path.exists() {
            records.push(json!({
                "type": "json_object_schema",
                "name": schema.name,
                "status": "missing",
                "path": path,
                "json_path": schema.json_path,
            }));
            continue;
        }
        let Some(value) = handlers::read_json_path(&path, &schema.json_path)? else {
            records.push(json!({
                "type": "json_object_schema",
                "name": schema.name,
                "status": "missing",
                "path": path,
                "json_path": schema.json_path,
            }));
            continue;
        };
        let Some(object) = value.as_object() else {
            records.push(json!({
                "type": "json_object_schema",
                "name": schema.name,
                "status": "warning",
                "warning": "value is not a JSON object",
                "path": path,
                "json_path": schema.json_path,
            }));
            continue;
        };
        let known_keys = schema.known_keys.iter().collect::<BTreeSet<_>>();
        let extra_keys = object
            .keys()
            .filter(|key| !known_keys.contains(key))
            .cloned()
            .collect::<Vec<_>>();
        records.push(json!({
            "type": "json_object_schema",
            "name": schema.name,
            "status": if extra_keys.is_empty() { "ok" } else { "warning" },
            "extra_keys": extra_keys,
            "warning": if extra_keys.is_empty() {
                Value::Null
            } else {
                json!(schema.extra_keys_warning.as_deref().unwrap_or(
                    "JSON object contains keys not listed in the app definition"
                ))
            },
            "path": path,
            "json_path": schema.json_path,
        }));
    }
    Ok(records)
}

fn doctor_definition_json_field_records(
    paths: &Paths,
    definition: &AppDefinition,
) -> Result<Vec<Value>> {
    let mut records = Vec::new();
    for field in &definition.doctor.json_fields {
        let path = match paths.expand_target_path(&field.path) {
            Ok(path) => path,
            Err(err) => {
                records.push(json!({
                    "type": "json_field",
                    "name": field.name,
                    "status": "warning",
                    "warning": err.to_string(),
                    "path_template": field.path,
                    "json_path": field.json_path,
                }));
                continue;
            }
        };
        if !path.exists() {
            records.push(json!({
                "type": "json_field",
                "name": field.name,
                "status": "missing",
                "path": path,
                "json_path": field.json_path,
            }));
            continue;
        }
        let value = match handlers::read_json_path(&path, &field.json_path) {
            Ok(Some(value)) => value,
            Ok(None) => {
                records.push(json!({
                    "type": "json_field",
                    "name": field.name,
                    "status": "missing",
                    "path": path,
                    "json_path": field.json_path,
                }));
                continue;
            }
            Err(err) => {
                records.push(json!({
                    "type": "json_field",
                    "name": field.name,
                    "status": "warning",
                    "warning": err.to_string(),
                    "path": path,
                    "json_path": field.json_path,
                }));
                continue;
            }
        };
        let rendered = if field.sensitive {
            "present".to_string()
        } else {
            render_doctor_json_value(&value)?
        };
        records.push(json!({
            "type": "json_field",
            "name": field.name,
            "status": "present",
            "value": rendered,
            "path": path,
            "json_path": field.json_path,
            "sensitive": field.sensitive,
        }));
        if let (Some(days), Some(value)) = (field.stale_after_days, value.as_str()) {
            if rfc3339_timestamp_is_older_than(value, days) {
                records.push(json!({
                    "type": "json_field_stale",
                    "name": field.name,
                    "status": "warning",
                    "warning": format!("older than {days} days"),
                    "path": path,
                    "json_path": field.json_path,
                }));
            }
        }
    }
    Ok(records)
}

fn doctor_definition(paths: &Paths, definition: &AppDefinition) -> Result<()> {
    doctor_definition_json_fields(paths, definition)?;
    doctor_definition_json_object_schemas(paths, definition)?;
    for (kind_name, kind) in &definition.kinds {
        for target in &kind.targets {
            let path = match paths.expand_target_path(&target.path) {
                Ok(path) => path,
                Err(err) => {
                    println!(
                        "definition_target\t{kind_name}\t{}\twarning: {err}",
                        target.handler
                    );
                    continue;
                }
            };
            let exists = if path.exists() { "exists" } else { "missing" };
            println!(
                "definition_target\t{kind_name}\t{}\t{exists}\t{}",
                target.handler,
                path.display()
            );
            if target.handler == "json_env_merge" && path.exists() {
                let present = target_present_json_env_keys(&path, target)?;
                if present.is_empty() {
                    println!("definition_managed_keys\t{kind_name}\tnone");
                } else {
                    println!(
                        "definition_managed_keys\t{kind_name}\t{}",
                        present.join(",")
                    );
                }
            }
            if let Some(json_path) = target.json_path.as_deref().filter(|_| path.exists()) {
                let status = if handlers::read_json_path(&path, json_path)?.is_some() {
                    "present"
                } else {
                    "missing"
                };
                println!(
                    "definition_json_path\t{kind_name}\t{}\t{status}\t{}",
                    target.handler, json_path
                );
            }
        }
        if kind.identity.is_some() {
            match read_live_identity(paths, definition, kind_name) {
                Ok(identity) if identity.is_empty() => {
                    println!("definition_identity\t{kind_name}\tmissing")
                }
                Ok(identity) => {
                    println!(
                        "definition_identity\t{kind_name}\t{}",
                        serde_json::to_string(&identity)?
                    );
                }
                Err(err) => println!("definition_identity\t{kind_name}\twarning: {err}"),
            }
        }
    }
    Ok(())
}

fn doctor_definition_json_object_schemas(paths: &Paths, definition: &AppDefinition) -> Result<()> {
    for schema in &definition.doctor.json_object_schemas {
        let path = match paths.expand_target_path(&schema.path) {
            Ok(path) => path,
            Err(err) => {
                println!(
                    "definition_json_object_schema\t{}\twarning: {err}\t{}\t{}",
                    schema.name, schema.path, schema.json_path
                );
                continue;
            }
        };
        if !path.exists() {
            println!(
                "definition_json_object_schema\t{}\tmissing\t{}\t{}",
                schema.name,
                path.display(),
                schema.json_path
            );
            continue;
        }
        let Some(value) = handlers::read_json_path(&path, &schema.json_path)? else {
            println!(
                "definition_json_object_schema\t{}\tmissing\t{}\t{}",
                schema.name,
                path.display(),
                schema.json_path
            );
            continue;
        };
        let Some(object) = value.as_object() else {
            println!(
                "definition_json_object_schema\t{}\twarning: value is not a JSON object\t{}\t{}",
                schema.name,
                path.display(),
                schema.json_path
            );
            continue;
        };
        let known_keys = schema.known_keys.iter().collect::<BTreeSet<_>>();
        let extra_keys = object
            .keys()
            .filter(|key| !known_keys.contains(key))
            .cloned()
            .collect::<Vec<_>>();
        if extra_keys.is_empty() {
            println!(
                "definition_json_object_schema\t{}\tok\t{}\t{}",
                schema.name,
                path.display(),
                schema.json_path
            );
        } else {
            let warning = schema
                .extra_keys_warning
                .as_deref()
                .unwrap_or("JSON object contains keys not listed in the app definition");
            println!(
                "definition_json_object_schema\t{}\twarning: {warning}; extra_keys={}\t{}\t{}",
                schema.name,
                extra_keys.join(","),
                path.display(),
                schema.json_path
            );
        }
    }
    Ok(())
}

fn doctor_definition_json_fields(paths: &Paths, definition: &AppDefinition) -> Result<()> {
    for field in &definition.doctor.json_fields {
        let path = match paths.expand_target_path(&field.path) {
            Ok(path) => path,
            Err(err) => {
                println!(
                    "definition_json_field\t{}\twarning: {err}\t{}\t{}",
                    field.name, field.path, field.json_path
                );
                continue;
            }
        };
        if !path.exists() {
            println!(
                "definition_json_field\t{}\tmissing\t{}\t{}",
                field.name,
                path.display(),
                field.json_path
            );
            continue;
        }
        let value = match handlers::read_json_path(&path, &field.json_path) {
            Ok(Some(value)) => value,
            Ok(None) => {
                println!(
                    "definition_json_field\t{}\tmissing\t{}\t{}",
                    field.name,
                    path.display(),
                    field.json_path
                );
                continue;
            }
            Err(err) => {
                println!(
                    "definition_json_field\t{}\twarning: {err}\t{}\t{}",
                    field.name,
                    path.display(),
                    field.json_path
                );
                continue;
            }
        };
        let rendered = if field.sensitive {
            "present".to_string()
        } else {
            render_doctor_json_value(&value)?
        };
        println!(
            "definition_json_field\t{}\t{}\t{}\t{}",
            field.name,
            rendered,
            path.display(),
            field.json_path
        );
        if let (Some(days), Some(value)) = (field.stale_after_days, value.as_str()) {
            if rfc3339_timestamp_is_older_than(value, days) {
                println!(
                    "definition_json_field\t{}\twarning: older than {days} days\t{}\t{}",
                    field.name,
                    path.display(),
                    field.json_path
                );
            }
        }
    }
    Ok(())
}

fn render_doctor_json_value(value: &Value) -> Result<String> {
    if let Some(value) = value.as_str() {
        Ok(value.to_string())
    } else {
        Ok(serde_json::to_string(value)?)
    }
}

fn rfc3339_timestamp_is_older_than(value: &str, days: i64) -> bool {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value) else {
        return false;
    };
    Utc::now()
        .signed_duration_since(parsed.with_timezone(&Utc))
        .num_days()
        > days
}

fn target_present_json_env_keys(
    path: &Path,
    target: &crate::app_definitions::TargetDefinition,
) -> Result<Vec<String>> {
    let Some(env) = handlers::read_json_path(path, target.json_path.as_deref().unwrap_or("$.env"))?
    else {
        return Ok(Vec::new());
    };
    Ok(target
        .managed_keys
        .iter()
        .filter(|key| {
            env.get(key.as_str())
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
        })
        .cloned()
        .collect())
}

fn split_kv(input: &str) -> Result<(&str, &str)> {
    let (key, value) = input
        .split_once('=')
        .ok_or_else(|| anyhow!("expected k=v: {input}"))?;
    validate_field_key(key)?;
    Ok((key, value))
}

fn validate_field_key(key: &str) -> Result<()> {
    if key.is_empty() || key.split('.').any(str::is_empty) {
        return Err(anyhow!("FieldInvalid: invalid field key {key:?}"));
    }
    Ok(())
}

fn read_secret_ref(paths: &Paths, input: &str) -> Result<String> {
    if input == "@stdin" {
        let mut value = String::new();
        io::stdin().read_to_string(&mut value)?;
        return Ok(value.trim_end_matches(['\r', '\n']).to_string());
    }
    if input == "@prompt" {
        return read_secret_prompt();
    }
    if let Some(name) = input.strip_prefix("@env:") {
        return std::env::var(name).with_context(|| format!("read env {name}"));
    }
    if let Some(path) = input.strip_prefix("@file:") {
        let path = validate_secret_file_path(paths, path)?;
        return Ok(fs::read_to_string(path)?
            .trim_end_matches(['\r', '\n'])
            .to_string());
    }
    Err(anyhow!(
        "secret value must use @stdin, @prompt, @env:NAME, or @file:PATH"
    ))
}

fn read_secret_prompt() -> Result<String> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "@prompt requires an interactive TTY; use @stdin, @env:NAME, or @file:PATH"
        ));
    }
    read_secret_prompt_from_tty()
}

#[cfg(unix)]
fn read_secret_prompt_from_tty() -> Result<String> {
    use std::os::fd::AsRawFd;

    eprint!("secret: ");
    io::stderr().flush()?;
    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let mut term = unsafe { std::mem::zeroed::<libc::termios>() };
    if unsafe { libc::tcgetattr(fd, &mut term) } != 0 {
        return Err(std::io::Error::last_os_error()).context("read terminal settings");
    }
    let original = term;
    term.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &term) } != 0 {
        return Err(std::io::Error::last_os_error()).context("disable terminal echo");
    }
    let mut value = String::new();
    let read_result = io::stdin().read_line(&mut value);
    let restore_result = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original) };
    eprintln!();
    if restore_result != 0 {
        return Err(std::io::Error::last_os_error()).context("restore terminal echo");
    }
    read_result?;
    Ok(value.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(not(unix))]
fn read_secret_prompt_from_tty() -> Result<String> {
    Err(anyhow!(
        "@prompt is not implemented on this platform; use @stdin, @env:NAME, or @file:PATH"
    ))
}

fn validate_secret_file_path(paths: &Paths, raw: &str) -> Result<PathBuf> {
    let path = expand_secret_file(raw, &paths.home);
    let canonical = path
        .canonicalize()
        .with_context(|| format!("read secret file {}", path.display()))?;
    let home = paths
        .home
        .canonicalize()
        .with_context(|| format!("canonicalize home {}", paths.home.display()))?;
    if !canonical.starts_with(&home) {
        return Err(anyhow!(
            "SecretFileOutsideHome: {} resolves to {}",
            path.display(),
            canonical.display()
        ));
    }
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "SecretFileInvalid: {} is not a file",
            canonical.display()
        ));
    }
    validate_secret_file_permissions(&canonical)?;
    Ok(canonical)
}

fn expand_secret_file(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}

#[cfg(unix)]
fn validate_secret_file_permissions(path: &Path) -> Result<()> {
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(anyhow!(
            "UnsafeSecretFilePermissions: {} has mode {:03o}; expected no group/other permissions",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_secret_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn backup_manifest_requires_app_stopped(paths: &Paths, app: &str, backup_id: &str) -> Result<bool> {
    let manifest = backup::load_validated_manifest(paths, app, backup_id)?;
    Ok(manifest
        .targets
        .iter()
        .any(|target| target.requires_app_stopped))
}

fn no_active_hint(app: &str) -> String {
    format!(
        "{app}: recommended next step is `any-switch import-current {app} <name>`, not `any-switch use <id>`, because use will not write back live state first and may overwrite it with a stale capture."
    )
}

fn cloud_sync_warning(paths: &Paths) -> &'static str {
    let switch_home = paths
        .switch_home
        .canonicalize()
        .unwrap_or_else(|_| paths.switch_home.clone());
    let home = paths
        .home
        .canonicalize()
        .unwrap_or_else(|_| paths.home.clone());
    let Ok(relative) = switch_home.strip_prefix(home) else {
        return "ok";
    };
    for marker in [
        &["Library", "Mobile Documents"][..],
        &["Dropbox"][..],
        &["OneDrive"][..],
        &["Google Drive"][..],
    ] {
        if path_starts_with_components(relative, marker) {
            return "warning: ANY_SWITCH_HOME appears to be under a cloud sync directory";
        }
    }
    "ok"
}

fn path_starts_with_components(path: &Path, expected: &[&str]) -> bool {
    let mut components = path.components();
    for expected in expected {
        match components.next() {
            Some(Component::Normal(actual)) if actual == *expected => {}
            _ => return false,
        }
    }
    true
}

#[cfg(unix)]
fn permission_warnings(paths: &Paths) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    collect_permission_warnings(&paths.switch_home, &mut warnings)?;
    Ok(warnings)
}

#[cfg(unix)]
fn collect_permission_warnings(path: &std::path::Path, warnings: &mut Vec<String>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(path)?;
    let mode = metadata.permissions().mode() & 0o777;
    if metadata.is_dir() {
        if mode & 0o077 != 0 {
            warnings.push(format!(
                "{} has mode {:03o}; expected 0700 or stricter",
                path.display(),
                mode
            ));
        }
        for entry in fs::read_dir(path)? {
            collect_permission_warnings(&entry?.path(), warnings)?;
        }
    } else if metadata.is_file() && mode & 0o077 != 0 {
        warnings.push(format!(
            "{} has mode {:03o}; expected 0600 or stricter",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn permission_warnings(_paths: &Paths) -> Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn custom_oauth_definition() -> AppDefinition {
        parse_definition(
            r#"
schema_version: 1
app:
  id: custom
  display_name: Custom
  definition_version: 1
process_probe:
  names: [custom]
kinds:
  oauth_capture:
    identity:
      handler: json_paths
      fields:
        tenant:
          path: $.tenant
          verify: required
        email:
          path: $.email
          verify: optional
    targets:
      - handler: file_capture
        path: ~/.custom/auth.json
        requires_app_stopped: true
"#,
        )
        .unwrap()
    }

    #[test]
    fn required_identity_matching_uses_definition_keys() {
        let definition = custom_oauth_definition();
        let mut expected = IndexMap::new();
        expected.insert("tenant".to_string(), Value::String("tenant-a".to_string()));
        expected.insert(
            "email".to_string(),
            Value::String("old@example.com".to_string()),
        );
        let mut actual = IndexMap::new();
        actual.insert("tenant".to_string(), Value::String("tenant-a".to_string()));
        actual.insert(
            "email".to_string(),
            Value::String("new@example.com".to_string()),
        );

        assert!(required_identity_matches_definition(
            &definition,
            "oauth_capture",
            &expected,
            &actual
        )
        .unwrap());

        actual.insert("tenant".to_string(), Value::String("tenant-b".to_string()));
        assert!(!required_identity_matches_definition(
            &definition,
            "oauth_capture",
            &expected,
            &actual
        )
        .unwrap());
    }

    #[test]
    fn optional_identity_warnings_use_definition_keys() {
        let definition = custom_oauth_definition();
        let mut profile = new_oauth_profile(
            "custom",
            "test",
            "custom-test".to_string(),
            IndexMap::new(),
            json!({"sources": []}),
        );
        profile
            .identity
            .insert("tenant".to_string(), Value::String("tenant-a".to_string()));
        profile.identity.insert(
            "email".to_string(),
            Value::String("old@example.com".to_string()),
        );
        let mut live = IndexMap::new();
        live.insert("tenant".to_string(), Value::String("tenant-b".to_string()));
        live.insert(
            "email".to_string(),
            Value::String("new@example.com".to_string()),
        );

        let warnings = optional_identity_warnings(&definition, &profile, &live);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["field"], "email");
    }

    #[test]
    fn read_live_identity_uses_definition_targets() {
        let home = tempfile::tempdir().unwrap();
        let auth_dir = home.path().join(".custom");
        fs::create_dir_all(&auth_dir).unwrap();
        fs::write(
            auth_dir.join("auth.json"),
            serde_json::to_vec(&json!({
                "tenant": "tenant-a",
                "email": "user@example.test"
            }))
            .unwrap(),
        )
        .unwrap();
        let paths = Paths {
            home: home.path().to_path_buf(),
            switch_home: home.path().join(".any-switch"),
        };
        let definition = custom_oauth_definition();

        let identity = read_live_identity(&paths, &definition, "oauth_capture").unwrap();

        assert_eq!(identity["tenant"], "tenant-a");
        assert_eq!(identity["email"], "user@example.test");
    }

    #[test]
    fn cloud_sync_warning_matches_only_known_home_roots() {
        let home = tempfile::tempdir().unwrap();
        let dropbox_switch_home = home.path().join("Dropbox").join(".any-switch");
        let icloud_switch_home = home
            .path()
            .join("Library")
            .join("Mobile Documents")
            .join(".any-switch");
        let false_positive_switch_home =
            home.path().join("Work-Dropbox-Archive").join(".any-switch");
        fs::create_dir_all(&dropbox_switch_home).unwrap();
        fs::create_dir_all(&icloud_switch_home).unwrap();
        fs::create_dir_all(&false_positive_switch_home).unwrap();

        assert!(cloud_sync_warning(&Paths {
            home: home.path().to_path_buf(),
            switch_home: dropbox_switch_home,
        })
        .starts_with("warning:"));
        assert!(cloud_sync_warning(&Paths {
            home: home.path().to_path_buf(),
            switch_home: icloud_switch_home,
        })
        .starts_with("warning:"));
        assert_eq!(
            cloud_sync_warning(&Paths {
                home: home.path().to_path_buf(),
                switch_home: false_positive_switch_home,
            }),
            "ok"
        );
    }
}

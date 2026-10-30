// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Exercise config/read through its real handler and ConfigBuilder, without starting a thread.

use super::ConfigRequestProcessor;
use crate::config_manager::ConfigManager;
use crate::error_code::INTERNAL_ERROR_CODE;
use crate::in_process::EmbeddedNetworkPolicy;
use crate::model_catalog::ModelCatalog;
use crate::outgoing_message::OutgoingMessageSender;
use anyhow::Result;
use codex_analytics::AnalyticsEventsClient;
use codex_app_server_protocol::ConfigLayerSource;
use codex_app_server_protocol::ConfigReadParams;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_config::SessionThreadConfig;
use codex_config::ThreadConfigContext;
use codex_config::ThreadConfigLoader;
use codex_config::ThreadConfigLoaderFuture;
use codex_config::ThreadConfigSource;
use codex_config::test_support::CloudConfigBundleFixture;
use codex_core::config::set_project_trust_level;
use codex_exec_server::EnvironmentManager;
use codex_http_client::NetworkPolicyDenied;
use codex_login::CodexAuth;
use codex_protocol::config_types::TrustLevel;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::timeout;

#[derive(Default)]
struct CountedThreadConfig {
    calls: AtomicUsize,
    contexts: Mutex<Vec<ThreadConfigContext>>,
    first_load_gate: Option<(Arc<Semaphore>, Arc<Semaphore>)>,
}

impl ThreadConfigLoader for CountedThreadConfig {
    fn load(
        &self,
        context: ThreadConfigContext,
    ) -> ThreadConfigLoaderFuture<'_, Vec<ThreadConfigSource>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.contexts.lock().unwrap().push(context);
        Box::pin(async move {
            if call == 0
                && let Some((entered, release)) = &self.first_load_gate
            {
                entered.add_permits(1);
                release.acquire().await.unwrap().forget();
            }
            // A second load would combine layers from one snapshot with features from another.
            Ok(vec![ThreadConfigSource::Session(SessionThreadConfig {
                features: [("memories".to_string(), call % 2 == 0)].into(),
                ..Default::default()
            })])
        })
    }
}

fn counted_manager(
    home: &Path,
    loader: Arc<CountedThreadConfig>,
    overrides: LoaderOverrides,
    cloud: CloudConfigBundleLoader,
) -> ConfigManager {
    ConfigManager::new(
        home.to_path_buf(),
        vec![(
            "model".to_string(),
            toml::Value::String("cli-model".to_string()),
        )],
        overrides,
        /*strict_config*/ false,
        cloud,
        Arg0DispatchPaths::default(),
        loader,
    )
}

async fn processor(manager: ConfigManager) -> Result<ConfigRequestProcessor> {
    // Infrastructure uses its own loader; only the actual read touches the counted loader.
    let initial = Arc::new(
        ConfigManager::without_managed_config_for_tests(manager.codex_home().to_path_buf())
            .load_non_project_config()
            .await?,
    );
    let threads = Arc::new(
        codex_core::test_support::thread_manager_with_models_provider_and_home(
            CodexAuth::from_api_key("dummy"),
            initial.model_provider.clone(),
            initial.codex_home.to_path_buf(),
            Arc::new(EnvironmentManager::default_for_tests()),
        ),
    );
    let (sender, _receiver) = tokio::sync::mpsc::channel(16);
    let analytics = AnalyticsEventsClient::disabled();
    let outgoing = Arc::new(OutgoingMessageSender::new(sender, analytics.clone()));
    let models = Arc::new(ModelCatalog::new(
        manager.clone(),
        None,
        initial,
        threads.get_models_manager(),
        threads.auth_manager(),
    ));
    Ok(ConfigRequestProcessor::new(
        outgoing, manager, threads, models, analytics,
    ))
}

#[tokio::test]
async fn config_read_loads_one_consistent_snapshot() -> Result<()> {
    for include_layers in [false, true] {
        let home = tempfile::tempdir()?;
        std::fs::write(home.path().join("config.toml"), "model = 'user-model'\n")?;
        let loader = Arc::new(CountedThreadConfig::default());
        let manager = counted_manager(
            home.path(),
            Arc::clone(&loader),
            LoaderOverrides::without_managed_config_for_tests(),
            CloudConfigBundleFixture::loader_with_enterprise_requirement(
                "allow_login_shell = false\n[feedback]\nenabled = false\n",
            ),
        );
        manager
            .extend_runtime_feature_enablement([("api_key_model_discovery".to_string(), true)])
            .unwrap();
        let handler = processor(manager).await?;
        let response = handler
            .read(ConfigReadParams {
                include_layers,
                cwd: None,
            })
            .await
            .expect("config read");

        assert_eq!(
            loader.calls.load(Ordering::SeqCst),
            1,
            "one load per config/read"
        );
        assert_eq!(response.config.model.as_deref(), Some("cli-model"));
        assert_eq!(
            response.config.additional["features"]["memories"],
            json!(true)
        );
        assert_eq!(
            response.config.additional["features"]["api_key_model_discovery"],
            json!(true)
        );
        assert_eq!(
            response.config.additional["allow_login_shell"],
            json!(false)
        );
        assert_eq!(
            response.config.additional["feedback"],
            json!({ "enabled": false })
        );
        assert_eq!(
            response.origins["model"].name,
            ConfigLayerSource::SessionFlags
        );
        assert_eq!(
            response.origins["features.memories"].name,
            ConfigLayerSource::SessionFlags
        );
        for exact in ["allow_login_shell", "feedback.enabled"] {
            assert!(
                !response.origins.contains_key(exact),
                "managed exact origin: {exact}"
            );
        }
        assert!(
            !response
                .origins
                .contains_key("features.api_key_model_discovery")
        );
        assert_eq!(response.layers.is_some(), include_layers);
        if let Some(layers) = response.layers {
            assert!(
                !layers
                    .iter()
                    .any(|layer| matches!(layer.name, ConfigLayerSource::PackagedDefaults { .. }))
            );
            assert!(layers.iter().any(|layer| {
                layer.name == ConfigLayerSource::SessionFlags
                    && layer.config["features"]["memories"] == json!(true)
            }));
            assert!(
                layers
                    .iter()
                    .any(|layer| matches!(layer.name, ConfigLayerSource::User { .. }))
            );
        }
        assert_eq!(loader.contexts.lock().unwrap()[0].cwd, None);
    }
    Ok(())
}

#[tokio::test]
async fn config_read_preserves_project_context_and_normalizes_relative_cwd() -> Result<()> {
    let project = tempfile::tempdir()?;
    let home = project.path().join("home");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(project.path().join(".git"))?;
    let dot_codex = project.path().join(".codex");
    std::fs::create_dir_all(&dot_codex)?;
    std::fs::write(
        dot_codex.join("config.toml"),
        "model_reasoning_effort = 'high'\n",
    )?;
    set_project_trust_level(&home, project.path(), TrustLevel::Trusted)?;
    let loader = Arc::new(CountedThreadConfig::default());
    let handler = processor(counted_manager(
        &home,
        Arc::clone(&loader),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleLoader::default(),
    ))
    .await?;

    let no_project = handler
        .read(ConfigReadParams {
            include_layers: true,
            cwd: None,
        })
        .await
        .expect("config read");
    assert_eq!(no_project.config.model_reasoning_effort, None);
    assert!(!no_project.origins.contains_key("model_reasoning_effort"));
    assert!(
        !no_project
            .layers
            .unwrap()
            .iter()
            .any(|layer| matches!(layer.name, ConfigLayerSource::Project { .. }))
    );
    assert_eq!(loader.contexts.lock().unwrap()[0].cwd, None);

    let absolute = AbsolutePathBuf::try_from(project.path().to_path_buf())?;
    let with_project = handler
        .read(ConfigReadParams {
            include_layers: true,
            cwd: Some(absolute.to_string_lossy().into_owned()),
        })
        .await
        .expect("project config read");
    assert_eq!(
        with_project.config.model_reasoning_effort,
        Some(codex_protocol::openai_models::ReasoningEffort::High)
    );
    assert_eq!(
        with_project.origins["model_reasoning_effort"].name,
        ConfigLayerSource::Project {
            dot_codex_folder: AbsolutePathBuf::try_from(dot_codex)?,
        }
    );
    assert_eq!(loader.contexts.lock().unwrap()[1].cwd, Some(absolute));

    // Both original acquisitions normalized relative cwd against the process cwd.
    let relative = handler
        .read(ConfigReadParams {
            include_layers: false,
            cwd: Some("relative-project".to_string()),
        })
        .await
        .expect("relative cwd remains compatible with the original handler");
    assert_eq!(relative.config.model.as_deref(), Some("cli-model"));
    assert_eq!(
        loader.contexts.lock().unwrap()[2].cwd,
        Some(AbsolutePathBuf::try_from(
            std::env::current_dir()?.join("relative-project")
        )?)
    );
    assert_eq!(
        loader.calls.load(Ordering::SeqCst),
        3,
        "each cwd form must acquire exactly one snapshot"
    );
    Ok(())
}

#[tokio::test]
async fn config_read_rejects_superseded_policy_and_account_snapshots() -> Result<()> {
    for account_changed in [true, false] {
        let home = tempfile::tempdir()?;
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let loader = Arc::new(CountedThreadConfig {
            first_load_gate: Some((Arc::clone(&entered), Arc::clone(&release))),
            ..Default::default()
        });
        let policy = EmbeddedNetworkPolicy::default();
        let manager = counted_manager(
            home.path(),
            Arc::clone(&loader),
            LoaderOverrides::without_managed_config_for_tests(),
            CloudConfigBundleFixture::loader_with_enterprise_config("model_context_window = 32768"),
        )
        .with_embedded_network_policy(policy.clone());
        let handler = processor(manager.clone()).await?;
        let pending = tokio::spawn(async move {
            handler
                .read(ConfigReadParams {
                    include_layers: true,
                    cwd: None,
                })
                .await
        });
        timeout(Duration::from_secs(5), entered.acquire())
            .await??
            .forget();
        if account_changed {
            // This is the same read-side revision/account invalidation used at account changes.
            policy.effective.policy().invalidate();
        } else {
            // Same network rules, different cloud snapshot: revision alone is insufficient.
            manager.clear_cloud_config_bundle_loader();
            manager.refresh_application_network_policy().await?;
        }
        release.add_permits(1);
        let error = timeout(Duration::from_secs(5), pending)
            .await??
            .unwrap_err();
        assert_eq!(error.code, INTERNAL_ERROR_CODE);
        assert!(
            error
                .message
                .contains(&NetworkPolicyDenied::Revoked.to_string()),
            "revocation cause missing from RPC error: {error:?}"
        );
        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn config_read_reloads_local_requirements_and_rejects_invalid_materialization() -> Result<()>
{
    let home = tempfile::tempdir()?;
    let requirements = home.path().join("requirements.toml");
    std::fs::write(&requirements, "allow_login_shell = true\n")?;
    let loader = Arc::new(CountedThreadConfig::default());
    let manager = counted_manager(
        home.path(),
        loader,
        LoaderOverrides {
            system_requirements_path: Some(requirements.clone()),
            ..LoaderOverrides::without_managed_config_for_tests()
        },
        CloudConfigBundleLoader::default(),
    );
    let handler = processor(manager.clone()).await?;
    let params = || ConfigReadParams {
        include_layers: false,
        cwd: None,
    };
    assert_eq!(
        handler
            .read(params())
            .await
            .expect("config read")
            .config
            .additional["allow_login_shell"],
        json!(true)
    );
    std::fs::write(&requirements, "allow_login_shell = false\n")?;
    let revoked = handler.read(params()).await.expect("config read");
    assert_eq!(revoked.config.additional["allow_login_shell"], json!(false));
    assert!(!revoked.origins.contains_key("allow_login_shell"));

    std::fs::write(&requirements, "[invalid requirements")?;
    let error = handler.read(params()).await.unwrap_err();
    assert_eq!(error.code, INTERNAL_ERROR_CODE);
    std::fs::write(&requirements, "")?;
    std::fs::write(home.path().join("config.toml"), "approval_policy = false\n")?;
    // The original handler's first read converted the real raw layer stack
    // directly. Preserve that exact typed error detail, not just a phase prefix.
    let original_typed_error = manager
        .load_config_layers(None)
        .await?
        .effective_config()
        .try_into::<codex_config::config_toml::ConfigToml>()
        .unwrap_err()
        .to_string();
    assert!(
        original_typed_error.contains("approval_policy"),
        "original raw typed error lost its key path: {original_typed_error}"
    );
    let error = handler.read(params()).await.unwrap_err();
    assert_eq!(error.code, INTERNAL_ERROR_CODE);
    assert!(
        error.message.contains(&original_typed_error),
        "original detail: {original_typed_error}\nactual RPC error: {error:?}"
    );

    std::fs::write(
        home.path().join("config.toml"),
        "model_catalog_json = 'missing-models.json'\n",
    )?;
    let error = handler.read(params()).await.unwrap_err();
    assert_eq!(error.code, INTERNAL_ERROR_CODE);
    std::fs::write(home.path().join("config.toml"), "")?;
    #[cfg(windows)]
    {
        // Opening this fixture-owned directory as a requirements file returns
        // Windows access denied. Do not modify ACLs or touch another profile.
        std::fs::remove_file(&requirements)?;
        std::fs::create_dir(&requirements)?;
        let original_io_error = manager.load_config_layers(None).await.unwrap_err();
        assert_eq!(
            original_io_error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "unexpected original requirements error: {original_io_error:?}"
        );
        let error = handler.read(params()).await.unwrap_err();
        assert_eq!(error.code, INTERNAL_ERROR_CODE);
        assert!(
            error.message.contains(&original_io_error.to_string()),
            "original I/O cause: {original_io_error}\nactual RPC error: {error:?}"
        );
        std::fs::remove_dir(&requirements)?;
        std::fs::write(&requirements, "")?;
    }
    assert!(
        handler.read(params()).await.is_ok(),
        "a failed read must not poison subsequent loads"
    );
    Ok(())
}

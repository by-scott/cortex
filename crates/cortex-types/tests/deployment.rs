use cortex_types::{
    ActorId, AuthContext, ClientId, DeploymentError, DeploymentEvidence, DeploymentPlan,
    DeploymentStep, OwnedScope, TenantId,
};

fn scope() -> OwnedScope {
    let context = AuthContext::new(
        TenantId::from_static("tenant-a"),
        ActorId::from_static("operator"),
        ClientId::from_static("cli"),
    );
    OwnedScope::private_for(&context)
}

#[test]
fn release_plan_requires_every_step_in_order() {
    let mut plan = DeploymentPlan::production_release(scope());

    assert!(!plan.release_ready());
    assert_eq!(
        plan.mark_passed(DeploymentStep::SmokeTest),
        Err(DeploymentError::OutOfOrder {
            step: DeploymentStep::SmokeTest
        })
    );

    for step in [
        DeploymentStep::Backup,
        DeploymentStep::Migrate,
        DeploymentStep::Install,
        DeploymentStep::SmokeTest,
        DeploymentStep::Package,
        DeploymentStep::Publish,
    ] {
        plan.mark_passed(step).unwrap();
    }

    assert!(plan.release_ready());
    assert!(!plan.rollback_required());
}

#[test]
fn failed_release_plan_requires_rollback_and_blocks_progress() {
    let mut plan = DeploymentPlan::production_release(scope());

    plan.mark_passed(DeploymentStep::Backup).unwrap();
    plan.mark_failed(DeploymentStep::Migrate).unwrap();

    assert!(plan.rollback_required());
    assert!(!plan.release_ready());
    assert_eq!(
        plan.mark_passed(DeploymentStep::Install),
        Err(DeploymentError::AlreadyFailed)
    );
    assert_eq!(
        plan.rollback_actions(),
        vec!["manual rollback required".to_string()]
    );

    plan.mark_rolled_back(DeploymentStep::Migrate).unwrap();

    assert!(plan.rollback_complete());
    assert!(!plan.rollback_required());
    assert!(!plan.release_ready());
    assert_eq!(
        plan.mark_rolled_back(DeploymentStep::Backup),
        Err(DeploymentError::NotFailed {
            step: DeploymentStep::Backup
        })
    );
    assert_eq!(
        plan.mark_passed(DeploymentStep::Install),
        Err(DeploymentError::OutOfOrder {
            step: DeploymentStep::Install
        })
    );
}

#[test]
fn release_plan_keeps_artifacts_and_explicit_rollback_actions() {
    let mut plan = DeploymentPlan::production_release(scope());

    plan.mark_passed_with_evidence(
        DeploymentStep::Backup,
        DeploymentEvidence::new("backup complete")
            .with_artifact("backups/cortex.bundle", "sha256:backup"),
    )
    .unwrap();
    plan.mark_failed_with_evidence(
        DeploymentStep::Migrate,
        DeploymentEvidence::new("migration failed")
            .with_artifact("logs/migrate.log", "sha256:log")
            .with_rollback("restore backups/cortex.bundle"),
    )
    .unwrap();

    let artifacts = plan.artifact_manifest();

    assert!(plan.rollback_required());
    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].path, "backups/cortex.bundle");
    assert_eq!(artifacts[1].checksum, "sha256:log");
    assert_eq!(
        plan.rollback_actions(),
        vec!["restore backups/cortex.bundle".to_string()]
    );
}

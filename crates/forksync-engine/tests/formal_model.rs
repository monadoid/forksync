use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use tla_connect::{
    ApalacheConfig, ApalacheMode, Driver, DriverError, ExtractState, State, Step, generate_traces,
    replay_trace_str, replay_traces, switch,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct BranchHead {
    base: i64,
    patches: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct SyncMachineState {
    #[serde(rename = "upstreamHead")]
    upstream_head: i64,
    #[serde(rename = "mainPatches")]
    main_patches: BTreeSet<String>,
    #[serde(rename = "liveHead")]
    live_head: BranchHead,
    #[serde(rename = "outputHead")]
    output_head: BranchHead,
    #[serde(rename = "authorBase")]
    author_base: i64,
    #[serde(rename = "lastProcessedUpstream")]
    last_processed_upstream: i64,
    #[serde(rename = "lastGoodSync")]
    last_good_sync: BranchHead,
    #[serde(rename = "historyLen")]
    history_len: i64,
    #[serde(rename = "lastHistoryOutcome")]
    last_history_outcome: String,
    outcome: String,
    #[serde(rename = "lockHeld")]
    lock_held: bool,
}

impl State for SyncMachineState {}

#[derive(Debug)]
struct SyncMachineDriver {
    state: SyncMachineState,
    update_output_branch: bool,
}

impl Default for SyncMachineDriver {
    fn default() -> Self {
        Self::new(true)
    }
}

impl SyncMachineDriver {
    fn new(update_output_branch: bool) -> Self {
        Self {
            state: SyncMachineState::default(),
            update_output_branch,
        }
    }
}

impl ExtractState<SyncMachineDriver> for SyncMachineState {
    fn from_driver(driver: &SyncMachineDriver) -> Result<Self, DriverError> {
        Ok(driver.state.clone())
    }
}

impl Driver for SyncMachineDriver {
    type State = SyncMachineState;

    fn step(&mut self, step: &Step) -> Result<(), DriverError> {
        switch!(step {
            "init" => {
                self.state = SyncMachineState::from_spec(&step.state)?;
                Ok(())
            },
            "UserCommit" => {
                self.state.main_patches.insert("patch_a".to_string());
                self.state.outcome = "Idle".to_string();
                Ok(())
            },
            "UpstreamAdvance" => {
                self.state.upstream_head += 1;
                self.state.outcome = "Idle".to_string();
                Ok(())
            },
            "StartSync" => {
                self.state.lock_held = true;
                self.state.outcome = "Running".to_string();
                Ok(())
            },
            "FinishNoChange" => {
                self.state.lock_held = false;
                self.state.outcome = "NoChange".to_string();
                Ok(())
            },
            "FinishSuccess" => {
                let generated = BranchHead {
                    base: self.state.upstream_head,
                    patches: self.state.main_patches.clone(),
                };
                self.state.live_head = generated.clone();
                if self.update_output_branch {
                    self.state.output_head = generated.clone();
                    self.state.author_base = self.state.upstream_head;
                }
                self.state.last_good_sync = generated;
                self.state.last_processed_upstream = self.state.upstream_head;
                self.state.history_len += 1;
                self.state.last_history_outcome = "Synced".to_string();
                self.state.outcome = "Synced".to_string();
                self.state.lock_held = false;
                Ok(())
            },
            "FinishFailedAgent" => {
                self.state.history_len += 1;
                self.state.last_history_outcome = "FailedAgent".to_string();
                self.state.outcome = "FailedAgent".to_string();
                self.state.lock_held = false;
                Ok(())
            }
        })
    }
}

#[test]
fn inline_itf_trace_replays_successful_sync_path() {
    let _ = replay_trace_str(SyncMachineDriver::default, default_success_trace())
        .expect("replay inline success trace");
}

#[test]
fn inline_itf_trace_replays_no_change_without_writing_history() {
    let _ = replay_trace_str(SyncMachineDriver::default, no_change_trace())
        .expect("replay inline no-change trace");
}

#[test]
fn inline_itf_trace_replays_live_only_success_path() {
    let _ = replay_trace_str(|| SyncMachineDriver::new(false), live_only_success_trace())
        .expect("replay inline live-only success trace");
}

#[test]
fn apalache_generated_traces_replay_when_apalache_is_available() {
    if !apalache_available() {
        eprintln!("Skipping Apalache-backed replay test because `apalache-mc` is not installed.");
        return;
    }

    let generated = generate_traces(&apalache_config("ConstInit"))
        .expect("generate traces from ForkSyncCore spec");
    assert!(
        !generated.traces.is_empty(),
        "expected Apalache to emit at least one trace"
    );

    let _ = replay_traces(SyncMachineDriver::default, &generated.traces)
        .expect("replay Apalache-generated traces");
}

#[test]
fn apalache_generated_live_only_traces_replay_when_apalache_is_available() {
    if !apalache_available() {
        eprintln!("Skipping Apalache-backed replay test because `apalache-mc` is not installed.");
        return;
    }

    let generated = generate_traces(&apalache_config("ConstInitLiveOnly"))
        .expect("generate live-only traces from ForkSyncCore spec");
    assert!(
        !generated.traces.is_empty(),
        "expected Apalache to emit at least one live-only trace"
    );

    let _ = replay_traces(|| SyncMachineDriver::new(false), &generated.traces)
        .expect("replay Apalache-generated live-only traces");
}

fn apalache_config(cinit: &str) -> ApalacheConfig {
    ApalacheConfig::builder()
        .spec(formal_spec_path())
        .inv("TraceComplete".to_string())
        .cinit(cinit.to_string())
        .max_traces(1usize)
        .max_length(5usize)
        .mode(ApalacheMode::Check)
        .apalache_bin(resolve_apalache_bin().expect("resolve apalache-mc path"))
        .build()
        .expect("build Apalache trace generation config")
}

fn formal_spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/tla/ForkSyncCore.tla")
}

fn apalache_available() -> bool {
    resolve_apalache_bin().is_some()
}

fn resolve_apalache_bin() -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let candidates = [
        "apalache-mc".to_string(),
        home.as_ref()
            .map(|home| home.join(".cargo/bin/apalache-mc").display().to_string())
            .unwrap_or_default(),
        home.as_ref()
            .map(|home| home.join(".local/bin/apalache-mc").display().to_string())
            .unwrap_or_default(),
    ];

    candidates.into_iter().find(|candidate| {
        if candidate.is_empty() {
            return false;
        }

        Command::new(candidate)
            .arg("typecheck")
            .arg(formal_spec_path())
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
}

fn default_success_trace() -> &'static str {
    r##"{
  "#meta": {
    "description": "ForkSync core success path",
    "varTypes": {
      "upstreamHead": "Int",
      "mainPatches": "Set(Str)",
      "liveHead": "(base: Int, patches: Set(Str))",
      "outputHead": "(base: Int, patches: Set(Str))",
      "authorBase": "Int",
      "lastProcessedUpstream": "Int",
      "lastGoodSync": "(base: Int, patches: Set(Str))",
      "historyLen": "Int",
      "lastHistoryOutcome": "Str",
      "outcome": "Str",
      "action_taken": "Str",
      "nondet_picks": "()",
      "lockHeld": "Bool"
    }
  },
  "vars": [
    "upstreamHead",
    "mainPatches",
    "liveHead",
    "outputHead",
    "authorBase",
    "lastProcessedUpstream",
    "lastGoodSync",
    "historyLen",
    "lastHistoryOutcome",
    "outcome",
    "action_taken",
    "nondet_picks",
    "lockHeld"
  ],
  "states": [
    {
      "#meta": { "index": 0 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "init",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 1 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "UserCommit",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 2 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "UpstreamAdvance",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 3 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Running",
      "action_taken": "StartSync",
      "nondet_picks": { "#tup": [] },
      "lockHeld": true
    },
    {
      "#meta": { "index": 4 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "1" }, "patches": { "#set": [ "patch_a" ] } },
      "outputHead": { "base": { "#bigint": "1" }, "patches": { "#set": [ "patch_a" ] } },
      "authorBase": { "#bigint": "1" },
      "lastProcessedUpstream": { "#bigint": "1" },
      "lastGoodSync": { "base": { "#bigint": "1" }, "patches": { "#set": [ "patch_a" ] } },
      "historyLen": { "#bigint": "1" },
      "lastHistoryOutcome": "Synced",
      "outcome": "Synced",
      "action_taken": "FinishSuccess",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    }
  ]
}"##
}

fn no_change_trace() -> &'static str {
    r##"{
  "#meta": {
    "description": "ForkSync no-change path",
    "varTypes": {
      "upstreamHead": "Int",
      "mainPatches": "Set(Str)",
      "liveHead": "(base: Int, patches: Set(Str))",
      "outputHead": "(base: Int, patches: Set(Str))",
      "authorBase": "Int",
      "lastProcessedUpstream": "Int",
      "lastGoodSync": "(base: Int, patches: Set(Str))",
      "historyLen": "Int",
      "lastHistoryOutcome": "Str",
      "outcome": "Str",
      "action_taken": "Str",
      "nondet_picks": "()",
      "lockHeld": "Bool"
    }
  },
  "vars": [
    "upstreamHead",
    "mainPatches",
    "liveHead",
    "outputHead",
    "authorBase",
    "lastProcessedUpstream",
    "lastGoodSync",
    "historyLen",
    "lastHistoryOutcome",
    "outcome",
    "action_taken",
    "nondet_picks",
    "lockHeld"
  ],
  "states": [
    {
      "#meta": { "index": 0 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "0" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "init",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 1 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "0" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Running",
      "action_taken": "StartSync",
      "nondet_picks": { "#tup": [] },
      "lockHeld": true
    },
    {
      "#meta": { "index": 2 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "0" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "NoChange",
      "action_taken": "FinishNoChange",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    }
  ]
}"##
}

fn live_only_success_trace() -> &'static str {
    r##"{
  "#meta": {
    "description": "ForkSync live-only success path",
    "varTypes": {
      "upstreamHead": "Int",
      "mainPatches": "Set(Str)",
      "liveHead": "(base: Int, patches: Set(Str))",
      "outputHead": "(base: Int, patches: Set(Str))",
      "authorBase": "Int",
      "lastProcessedUpstream": "Int",
      "lastGoodSync": "(base: Int, patches: Set(Str))",
      "historyLen": "Int",
      "lastHistoryOutcome": "Str",
      "outcome": "Str",
      "action_taken": "Str",
      "nondet_picks": "()",
      "lockHeld": "Bool"
    }
  },
  "vars": [
    "upstreamHead",
    "mainPatches",
    "liveHead",
    "outputHead",
    "authorBase",
    "lastProcessedUpstream",
    "lastGoodSync",
    "historyLen",
    "lastHistoryOutcome",
    "outcome",
    "action_taken",
    "nondet_picks",
    "lockHeld"
  ],
  "states": [
    {
      "#meta": { "index": 0 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "init",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 1 },
      "upstreamHead": { "#bigint": "0" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "UserCommit",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 2 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Idle",
      "action_taken": "UpstreamAdvance",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    },
    {
      "#meta": { "index": 3 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "-1" },
      "lastGoodSync": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "historyLen": { "#bigint": "0" },
      "lastHistoryOutcome": "None",
      "outcome": "Running",
      "action_taken": "StartSync",
      "nondet_picks": { "#tup": [] },
      "lockHeld": true
    },
    {
      "#meta": { "index": 4 },
      "upstreamHead": { "#bigint": "1" },
      "mainPatches": { "#set": [ "patch_a" ] },
      "liveHead": { "base": { "#bigint": "1" }, "patches": { "#set": [ "patch_a" ] } },
      "outputHead": { "base": { "#bigint": "0" }, "patches": { "#set": [] } },
      "authorBase": { "#bigint": "0" },
      "lastProcessedUpstream": { "#bigint": "1" },
      "lastGoodSync": { "base": { "#bigint": "1" }, "patches": { "#set": [ "patch_a" ] } },
      "historyLen": { "#bigint": "1" },
      "lastHistoryOutcome": "Synced",
      "outcome": "Synced",
      "action_taken": "FinishSuccess",
      "nondet_picks": { "#tup": [] },
      "lockHeld": false
    }
  ]
}"##
}

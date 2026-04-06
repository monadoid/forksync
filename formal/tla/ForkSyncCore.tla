---- MODULE ForkSyncCore ----
EXTENDS Integers, TLC

CONSTANTS
    \* @type: Set(Str);
    PatchIds,
    \* @type: Int;
    MaxUpstream,
    \* @type: Int;
    MaxHistory,
    \* @type: Bool;
    UpdateOutputBranch

VARIABLES
    \* @type: Int;
    upstreamHead,
    \* @type: Set(Str);
    mainPatches,
    \* @type: { base: Int, patches: Set(Str) };
    liveHead,
    \* @type: { base: Int, patches: Set(Str) };
    outputHead,
    \* @type: Int;
    authorBase,
    \* @type: Int;
    lastProcessedUpstream,
    \* @type: { base: Int, patches: Set(Str) };
    lastGoodSync,
    \* @type: Int;
    remoteVersion,
    \* @type: Int;
    observedRemoteVersion,
    \* @type: Int;
    historyLen,
    \* @type: Str;
    lastHistoryOutcome,
    \* @type: Str;
    outcome,
    \* @type: Str;
    action_taken,
    \* @type: Seq(Str);
    nondet_picks,
    \* @type: Bool;
    lockHeld

Vars ==
    << upstreamHead, mainPatches, liveHead, outputHead, authorBase,
       lastProcessedUpstream, lastGoodSync, remoteVersion, observedRemoteVersion,
       historyLen, lastHistoryOutcome, outcome, action_taken,
       nondet_picks, lockHeld >>

Idle == "Idle"
Running == "Running"
NoChange == "NoChange"
Synced == "Synced"
FailedAgent == "FailedAgent"
StaleRun == "StaleRun"
NoRecordedOutcome == "None"
RemoteVersionLimit == 2 * (MaxUpstream + MaxHistory + 2)

Init ==
    /\ upstreamHead = 0
    /\ mainPatches = {}
    /\ liveHead = [base |-> 0, patches |-> {}]
    /\ outputHead = [base |-> 0, patches |-> {}]
    /\ authorBase = 0
    /\ lastProcessedUpstream = -1
    /\ lastGoodSync = [base |-> 0, patches |-> {}]
    /\ remoteVersion = 0
    /\ observedRemoteVersion = -1
    /\ historyLen = 0
    /\ lastHistoryOutcome = NoRecordedOutcome
    /\ outcome = Idle
    /\ action_taken = "init"
    /\ nondet_picks = <<>>
    /\ lockHeld = FALSE

ConstInit ==
    /\ PatchIds = {"patch_a", "patch_b"}
    /\ MaxUpstream = 2
    /\ MaxHistory = 6
    /\ UpdateOutputBranch = TRUE

ConstInitLiveOnly ==
    /\ PatchIds = {"patch_a", "patch_b"}
    /\ MaxUpstream = 2
    /\ MaxHistory = 6
    /\ UpdateOutputBranch = FALSE

UserCommit ==
    /\ ~lockHeld
    /\ \E p \in PatchIds \ mainPatches:
        mainPatches' = mainPatches \cup {p}
    /\ outcome' = Idle
    /\ action_taken' = "UserCommit"
    /\ nondet_picks' = <<>>
    /\ UNCHANGED << upstreamHead, liveHead, outputHead, authorBase,
                    lastProcessedUpstream, lastGoodSync, remoteVersion, observedRemoteVersion, historyLen,
                    lastHistoryOutcome, lockHeld >>

UpstreamAdvance ==
    /\ ~lockHeld
    /\ upstreamHead < MaxUpstream
    /\ upstreamHead' = upstreamHead + 1
    /\ outcome' = Idle
    /\ action_taken' = "UpstreamAdvance"
    /\ nondet_picks' = <<>>
    /\ UNCHANGED << mainPatches, liveHead, outputHead, authorBase,
                    lastProcessedUpstream, lastGoodSync, remoteVersion, observedRemoteVersion, historyLen,
                    lastHistoryOutcome, lockHeld >>

StartSync ==
    /\ ~lockHeld
    /\ lockHeld' = TRUE
    /\ observedRemoteVersion' = remoteVersion
    /\ outcome' = Running
    /\ action_taken' = "StartSync"
    /\ nondet_picks' = <<>>
    /\ UNCHANGED << upstreamHead, mainPatches, liveHead, outputHead,
                    authorBase, lastProcessedUpstream, lastGoodSync,
                    remoteVersion, historyLen, lastHistoryOutcome >>

CompetingPublish ==
    /\ lockHeld
    /\ remoteVersion < RemoteVersionLimit
    /\ remoteVersion' = remoteVersion + 1
    /\ outcome' = Running
    /\ action_taken' = "CompetingPublish"
    /\ nondet_picks' = <<>>
    /\ UNCHANGED << upstreamHead, mainPatches, liveHead, outputHead,
                    authorBase, lastProcessedUpstream, lastGoodSync,
                    observedRemoteVersion, historyLen, lastHistoryOutcome, lockHeld >>

FinishNoChange ==
    /\ lockHeld
    /\ upstreamHead = lastProcessedUpstream
    /\ outcome' = NoChange
    /\ action_taken' = "FinishNoChange"
    /\ nondet_picks' = <<>>
    /\ lockHeld' = FALSE
    /\ UNCHANGED << upstreamHead, mainPatches, liveHead, outputHead,
                    authorBase, lastProcessedUpstream, lastGoodSync,
                    remoteVersion, observedRemoteVersion, historyLen, lastHistoryOutcome >>

FinishSuccess ==
    /\ lockHeld
    /\ remoteVersion = observedRemoteVersion
    /\ historyLen < MaxHistory
    /\ remoteVersion < RemoteVersionLimit
    /\ liveHead' = [base |-> upstreamHead, patches |-> mainPatches]
    /\ outputHead' =
        IF UpdateOutputBranch
        THEN [base |-> upstreamHead, patches |-> mainPatches]
        ELSE outputHead
    /\ authorBase' =
        IF UpdateOutputBranch
        THEN upstreamHead
        ELSE authorBase
    /\ lastProcessedUpstream' = upstreamHead
    /\ lastGoodSync' = [base |-> upstreamHead, patches |-> mainPatches]
    /\ remoteVersion' = remoteVersion + 1
    /\ historyLen' = historyLen + 1
    /\ lastHistoryOutcome' = Synced
    /\ outcome' = Synced
    /\ action_taken' = "FinishSuccess"
    /\ nondet_picks' = <<>>
    /\ lockHeld' = FALSE
    /\ UNCHANGED << upstreamHead, mainPatches, observedRemoteVersion >>

FinishFailedAgent ==
    /\ lockHeld
    /\ historyLen < MaxHistory
    /\ outcome' = FailedAgent
    /\ historyLen' = historyLen + 1
    /\ lastHistoryOutcome' = FailedAgent
    /\ action_taken' = "FinishFailedAgent"
    /\ nondet_picks' = <<>>
    /\ lockHeld' = FALSE
    /\ UNCHANGED << upstreamHead, mainPatches, liveHead, outputHead,
                    authorBase, lastProcessedUpstream, lastGoodSync,
                    remoteVersion, observedRemoteVersion >>

FinishStaleRun ==
    /\ lockHeld
    /\ remoteVersion /= observedRemoteVersion
    /\ outcome' = StaleRun
    /\ action_taken' = "FinishStaleRun"
    /\ nondet_picks' = <<>>
    /\ lockHeld' = FALSE
    /\ UNCHANGED << upstreamHead, mainPatches, liveHead, outputHead,
                    authorBase, lastProcessedUpstream, lastGoodSync,
                    remoteVersion, observedRemoteVersion, historyLen, lastHistoryOutcome >>

Next ==
    \/ UserCommit
    \/ UpstreamAdvance
    \/ StartSync
    \/ CompetingPublish
    \/ FinishNoChange
    \/ FinishSuccess
    \/ FinishFailedAgent
    \/ FinishStaleRun

Spec ==
    Init /\ [][Next]_Vars

TraceComplete ==
    outcome /= Synced

OutcomeIsKnown ==
    outcome \in {Idle, Running, NoChange, Synced, FailedAgent, StaleRun}

LockMatchesRunning ==
    lockHeld <=> outcome = Running

SuccessUpdatesLiveAndState ==
    outcome = Synced =>
        /\ liveHead.base = upstreamHead
        /\ lastGoodSync = liveHead
        /\ lastProcessedUpstream = upstreamHead
        /\ IF UpdateOutputBranch
           THEN /\ outputHead = liveHead
                /\ authorBase = upstreamHead
           ELSE TRUE

FailurePreservesGeneratedState ==
    outcome = FailedAgent =>
        /\ lastGoodSync = liveHead
        /\ ~lockHeld

StaleRunPreservesGeneratedState ==
    outcome = StaleRun =>
        /\ lastGoodSync = liveHead
        /\ ~lockHeld

RecordedHistoryOutcomeIsKnown ==
    lastHistoryOutcome \in {NoRecordedOutcome, Synced, FailedAgent}

TerminalOutcomesAppendHistory ==
    /\ outcome = Synced => /\ historyLen > 0
                           /\ lastHistoryOutcome = Synced
    /\ outcome = FailedAgent => /\ historyLen > 0
                                /\ lastHistoryOutcome = FailedAgent

NoChangeDoesNotWriteHistory ==
    outcome = NoChange => lastHistoryOutcome /= NoChange

TypeInvariant ==
    /\ upstreamHead \in 0..MaxUpstream
    /\ mainPatches \subseteq PatchIds
    /\ liveHead.base \in 0..MaxUpstream
    /\ outputHead.base \in 0..MaxUpstream
    /\ authorBase \in 0..MaxUpstream
    /\ lastProcessedUpstream \in -1..MaxUpstream
    /\ lastGoodSync.base \in 0..MaxUpstream
    /\ remoteVersion \in 0..RemoteVersionLimit
    /\ observedRemoteVersion \in -1..RemoteVersionLimit
    /\ historyLen \in 0..MaxHistory
    /\ action_taken \in {"init", "UserCommit", "UpstreamAdvance", "StartSync", "CompetingPublish", "FinishNoChange", "FinishSuccess", "FinishFailedAgent", "FinishStaleRun"}

LiveOnlyDoesNotAdvanceOutput ==
    /\ ~UpdateOutputBranch
    /\ outcome = Synced
    => /\ outputHead = [base |-> 0, patches |-> {}]
       /\ authorBase = 0

====

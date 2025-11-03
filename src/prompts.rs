pub const DEFAULT_WORKER_PROMPT: &str = "@{checklist} Do the thing.";

pub const STOP_CONFIRMATION_PROMPT: &str = r#"I detected that the previous response emitted the stop/completion token "{completion_token}".
Re-open @{checklist}. Confirm that every requirement and task is complete, the code builds cleanly, and all changes are committed.
Emit "{completion_token}" again on a line by itself at the very end ONLY if the loop should end. 
If ANYTHING remains, do NOT emit the token. Instead, briefly note what's left (one line), then continue normal work.
"#;

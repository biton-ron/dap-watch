# Dap-Watch

## Left To Do

### [DapStream]

[X] Complete DapStream buffer parsing  
[X] Complete DapMessage parser  
[X] Complete DapStream read loop to result in DapMessages  
[ ] Write logic

### [DapAdapter]

[ ] Trait - Spawn  
[ ] Trait - Kill  
[ ] Trait - Replay  
[ ] Adapter for Rust  
[ ] Adapter for Go

### [MainLoop]

[ ] Establish initial bi-directional pass forward  
[ ] Store state for state-related requests  
[ ] Replay stored state when re-spawning a debug adapter

### [FileWatcher]

[ ] Configuration should include files to include, exclude, and gitignore support  
[ ] Should notify the main loop when file updates were made

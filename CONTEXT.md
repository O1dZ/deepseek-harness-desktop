# DeepSeek Harness Desktop

This context names the user-facing concepts of the desktop companion for DeepSeek Harness. It keeps product language consistent across the interface and documentation.

## Language

**Harness Desktop**:
The installable desktop application through which a person uses DeepSeek Harness without opening a terminal.
_Avoid_: GUI wrapper, Codex clone, desktop shell

**Workspace**:
A local folder whose files and commands are available to a Harness task.
_Avoid_: Project, repository, working directory

**Task**:
A saved interaction with DeepSeek Harness within one workspace, including its conversation and activity history.
_Avoid_: Chat, thread, session

**Harness Runtime**:
The local DeepSeek Harness service used by Harness Desktop to execute tasks.
_Avoid_: Backend, web server, CLI process

**Edition**:
One distribution of Harness Desktop with a particular set of host prerequisites; editions provide the same product capabilities and read the same user data.
_Avoid_: Version, tier, plan

**Lite Edition**:
The small Harness Desktop edition intended for a development-ready Windows computer that already provides the required runtimes.
_Avoid_: Developer edition, portable edition, minimal build

**Full Edition**:
The self-contained Harness Desktop edition intended to run without separately preparing development runtimes.
_Avoid_: Pro edition, paid edition, bundled build

**Activity**:
A visible record of reasoning, tool use, command execution, or another operation performed during a task.
_Avoid_: Log entry, event, trace

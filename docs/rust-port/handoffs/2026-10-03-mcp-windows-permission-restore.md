# Exact token permission restoration in the Windows fixture

Native MCP run 37125601804 at 2f778248063dc88d3d0f51430660bb020005f7cb passed actual Go/Rust Windows bind-error spelling, complete HTTP headers, token owner/group/DACL/protection, rotation, read-only replacement failure retention, Ctrl-C and Ctrl-Break on amd64 and arm64. Required Windows Clippy in run 37125601802/job 111210199968 rejected `Permissions::set_readonly(false)` in fixture cleanup; full native Rust run 37126504276 reproduced the lint on both Windows architectures.

Cleanup now saves the original Permissions before making the owned token read-only and restores that exact saved value afterwards. No global lint exemption, Unix chmod behavior, production permissions or test acceptance changes. The failed lint runs remain retained; the corrected head requires fresh checks.

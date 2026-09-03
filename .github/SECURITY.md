# Security policy

## Supported versions

This tool is in a `0.x` series and every release is a pre-release. There is
no long-term support branch: a fix ships in a new release cut from `main`, and
older versions are not patched. If you hit a security issue, expect the fix in
the next release rather than a backport.

## What is in scope

This tool drives Apple's private `GPUToolsReplay.framework` through the
`gputools-replay-hl` crate and writes what the replayer serves into KTX2
files, so the interesting failures are on that path:

- A written file whose header disagrees with its payload, or whose payload is
  not the bytes the replayer served (a wrong value presented as trustworthy,
  not merely a texture the tool declines to write).
- A panic or memory-safety issue reachable from a capture bundle or a replay
  reply. Soundness of the FFI boundary itself belongs to `gputools-replay`;
  report it there.
- The release and publishing path: the publishing workflow, its
  trusted-publishing configuration, or an archive / tag that does not match the
  source it claims to build from.

## What is not in scope

- Anything that requires a modified or hostile build of the private framework,
  or a modified or hostile OS. The trust boundary is Apple's shipping framework
  on a stock system.
- Behavior on unsupported macOS (older than macOS 26), where the engine
  refuses to build by design.
- Anything that can only be reproduced with a capture you cannot share. Without
  a repro there is nothing to fix; see below for what to send.

## Reporting a vulnerability

Please report privately through GitHub's private vulnerability reporting: open
the repository's **Security** tab and choose **Report a vulnerability**. Do not
open a public issue for a suspected vulnerability.

To let a fix happen quickly, include:

- the smallest repro you can manage;
- `rustc -Vv`;
- `sw_vers -productVersion`;
- the tool version (or git SHA) and the `engine` line from its `manifest.json`;
- if it is shareable, the capture that triggers it.

This is a one-person project. Replies are best-effort and usually land within a
few days.

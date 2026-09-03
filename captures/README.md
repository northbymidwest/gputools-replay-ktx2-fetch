# Captures

Not committed: each is tens of megabytes and reproducible. Regenerate all
of them with `fixtures/build-all.sh`; see `fixtures/README.md` for what
each contains. The oracle tests (`tools/oracle.sh`) look for
`captures/<name>.gputrace` and skip, naming the script, when one is
missing.

`sample.gputrace` and `retroarch-trace.gputrace` are third-party traces no
test needs. tool-2's regression figures on them: 4 files from sample; 182
records on retroarch, 10 of them RGBA32Float with min -1 and max 46250.

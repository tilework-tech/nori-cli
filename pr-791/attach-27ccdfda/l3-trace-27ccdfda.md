# `@` on a joined `nori attach --host` at 27ccdfda

Separate synthetic host and client homes. The host folder holds `HostOnlyQuartz7c.txt`, sessions amber and birch each hold one unique file, and the client folder holds `ClientOnlyFern9.txt`. A relay between client and host records each `_nori/session/find_paths` request and reply.

| step | typed | session shown | host request → root, paths | popup |
|---|---|---|---|---|
| no session | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| no session | `@AmberOnlyL` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session | `@ClientOnlyF` | (no session) | (none: host folder) → hostdir [] | no matches |
| no session, same text again | `@HostOnlyQ` | (no session) | (none: host folder) → hostdir ['HostOnlyQuartz7c.txt'] | HostOnlyQuartz7c.txt |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to amber | `@BirchOnlyW` | amber | amber → amber [] | no matches |
| right after Alt+S to amber | `@AmberOnlyL` | amber | amber → amber ['AmberOnlyLark3.rs'] | AmberOnlyLark3.rs |
| amber, same text again | `@AmberOnlyL` | amber | amber → amber ['AmberOnlyLark3.rs'] | AmberOnlyLark3.rs |
| right after Alt+S to birch | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |
| right after Alt+S to birch | `@AmberOnlyL` | birch | birch → birch [] | no matches |
| birch, same text again | `@BirchOnlyW` | birch | birch → birch ['BirchOnlyWren5.rs'] | BirchOnlyWren5.rs |

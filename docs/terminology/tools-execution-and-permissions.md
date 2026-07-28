# Tools, Execution, and Permissions

## Actions and reports

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Tool call** | An identified action a model asks its agent to perform and the agent reports through ACP. | Command, permission |
| **Tool call update** | An ACP notification changing an existing **tool call** under the same ID. | New call, result |
| **Shell command** | A command line or argument vector executed as a process, from a tool call or a user. | Tool call |
| **Patch** | Structured file additions, deletions, updates, or moves. | Diff, edit |

## Permission

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **ACP permission request** | A correlated agent-to-client request presenting options for one tool call. | Approval, authorization |
| **Permission option** | An identified agent-supplied allow or reject choice, optionally remembered. | Outcome |
| **Permission outcome** | An ACP response selecting an option or reporting prompt-turn cancellation. | Approval, grant |
| **User approval** | A human's affirmative choice for one **ACP permission request**. | Outcome, policy |

## Controls and lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Tool call status** | The ACP lifecycle value `pending`, `in_progress`, `completed`, or `failed` reported for a tool call. | Turn status |
| **Approval policy** | Nori configuration deciding whether to surface a permission request or auto-select an allow option. | Sandbox policy, execution policy |
| **Sandbox policy** | Resolved filesystem and network restrictions for Nori's local sandbox execution. | Approval policy, permission |
| **Execution policy** | Command rules classifying argument vectors as allow, prompt, or forbidden in execpolicy tooling, not Nori's live ACP permission path. | Approval policy |
| **Cancellation** | A client signal to stop the active turn, cancel pending permission requests, and abort agent work. | Rejection, failure |

## Relationships

- A **shell command** or **patch** may be the payload of a **tool call**, but neither is itself an ACP tool call.
- A **tool call update** reports progress or results; it does not request execution.
- An **ACP permission request** may leave its tool call `pending`; its selected **permission outcome** may allow or reject.
- Only an affirmative human selection is **user approval**.
- **Approval policy** governs consultation, while **sandbox policy** constrains an execution environment.
- External ACP-agent tools bypass Nori's sandbox executor, and provider-internal tools may never cross an ACP permission boundary.
- **Cancellation** is not complete until the correlated prompt response reports the cancelled stop reason; final tool updates may arrive first.

## Example dialogue

> **Dev:** "The agent reported a **tool call** containing `cargo test`; did Nori run it?"
>
> **Domain expert:** "No. `cargo test` is the **shell command**; the external agent owns execution."
>
> **Dev:** "If Nori shows an **ACP permission request**, does selecting Reject mean the tool failed?"
>
> **Domain expert:** "No. That is a rejecting **permission outcome**; `failed` is a **tool call status**, while **cancellation** belongs to the prompt turn."

## Flagged ambiguities

- "Tool" may mean a capability, invocation, or UI row; reserve **tool call** for the identified ACP invocation.
- "Approval" conflates the request, a human decision, an ACP outcome, and policy; name the exact concept.
- ACP cancellation prose says clients should mark unfinished calls `cancelled`, but ACP's **tool call status** enumeration has no `cancelled` value; do not invent that wire status.
- "Patch" and "diff" differ: a diff can report file effects without proving Nori applied a patch.
- "Policy" alone is unusably broad; say **approval policy**, **sandbox policy**, or **execution policy**.

# Vendor Map

Public v0.1 has no NosisTech-hosted data processor. The operator chooses which third-party
provider or MCP service receives task data.

| Vendor | Purpose | Data sent | Cost/expiry posture | Exit path |
|---|---|---|---|---|
| DeepSeek | Direct model API | Prompt/system/history/tool data needed for the turn | User-billed; catalog prices carry a confidence label, no expiry | Remove key and select another route |
| Moonshot/Kimi | Direct model API | Same; multimodal inputs on capable routes | User-billed; catalog prices carry a confidence label, no expiry | Remove key and select another route |
| Xiaomi/MiMo | Direct model API | Same | User-billed; catalog prices carry a confidence label, no expiry | Remove key and select another route |
| Z.AI/GLM | Direct model API | Same | User-billed; some routes currently listed free; free status not monitored | Remove key and select another route |
| User-configured MCP service | Optional tool discovery/calls | Tool arguments and protocol metadata | Service-specific | Remove it from user-global config |
| GitHub (intended) | Public source, CI, releases, security intake | Repository contents and CI logs | Plan-specific | Git is portable; mirror elsewhere |
| crates.io | Rust dependencies | Package names/versions during fetch | Public registry | Lockfile plus source policy; vendor if required |

Remote notification vendors are intentionally absent from public v0.1.

## Sub-Agent Invocation

- If the runtime environment supports sub-agents / Tasks / parallel agents, they should be actively used for independently decomposable work:
    - Repetitive parallel tasks (multi-source retrieval, multi-file verification, multi-approach exploration);
    - Massive output tasks (large files, long logs, historical sessions, paper or batch report extraction);
    - Preparatory tasks (finding/installing required skills, inventorying tools and dependencies, organizing evidence indexes);
    - Asynchronous waiting tasks (long-running commands, testing, building, downloading, index generation);
    - Independent review tasks (omission checks, source verification, risk re-assessment).
- The main thread is responsible for task decomposition, scope definition, result integration, and final judgment.
- Do not completely outsource core judgments that the next step immediately depends on, or operations with side effects, to a sub-agent.
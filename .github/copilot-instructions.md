Log messages must begin with a capital letter and should not end with a period. Check that all log statements follow this pattern across debug, info, warn, and error levels.

Log messages should generally be static text with dynamic content provided through fields. Error messages should be included in the log message text even if they contain dynamic content. Ensure fields use snake_case naming and are consistent across related log statements.

Spans must be short-lived and should not leak events to long-running tasks. Verify that spans use INFO level for proper log decoration, have snake_case names that represent the short-lived action being performed, and are properly scoped to avoid memory leaks in long-running threads.

Ensure appropriate log levels are used: ERROR for threats to AC operation or security issues, WARN for impacts to AC behavior without breaking functionality or future potential problems, INFO for general operational information, DEBUG for internal details, and TRACE for very detailed troubleshooting information.

Code should be self-documenting and explicit in its intent. Avoid clever or overly concise code that sacrifices readability. Function and variable names should clearly indicate their purpose without requiring additional context or comments to understand their role.

Keep functions and methods focused on a single responsibility. Avoid deeply nested control structures, excessive function parameters, and complex conditional logic. Break down complex operations into smaller, well-named helper functions that can be easily understood and tested independently.

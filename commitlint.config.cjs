// SPDX-License-Identifier: AGPL-3.0-or-later
// Conventional Commits (CONTRIBUTING.md). Checked in CI by the `commitlint` job.
module.exports = {
  extends: ["@commitlint/config-conventional"],
  rules: {
    // Allow long URLs / trailers such as Signed-off-by in bodies.
    "body-max-line-length": [0, "always"],
    "footer-max-line-length": [0, "always"],
  },
};

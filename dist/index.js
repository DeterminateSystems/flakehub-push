// ts/index.ts
import * as actionsCore from "@actions/core";
import * as actionsExec from "@actions/exec";
import { IdsToolbox, inputs } from "detsys-ts";
var EVENT_EXECUTION_FAILURE = "execution_failure";
var VISIBILITY_OPTIONS = ["public", "unlisted", "private"];
var FlakeHubPushAction = class {
  constructor() {
    const options = {
      name: "flakehub-push",
      fetchStyle: "nix-style",
      diagnosticsUrl: new URL(
        "https://install.determinate.systems/flakehub-push/telemetry"
      ),
      legacySourcePrefix: "flakehub-push",
      requireNix: "ignore"
    };
    this.idslib = new IdsToolbox(options);
    const visibility = this.verifyVisibility();
    this.visibility = visibility;
    this.name = inputs.getStringOrNull("name");
    this.repository = inputs.getString("repository");
    this.mirror = inputs.getBool("mirror");
    this.directory = inputs.getString("directory");
    this.gitRoot = inputs.getString("git-root");
    this.tag = inputs.getString("tag");
    this.rollingMinor = inputs.getNumberOrNull("rolling-minor");
    this.rolling = inputs.getBool("rolling");
    this.host = inputs.getString("host");
    this.logDirectives = inputs.getString("log-directives");
    this.logger = inputs.getString("logger");
    this.gitHubToken = inputs.getString("github-token");
    this.extraLabels = inputs.getString("extra-labels") === "" ? inputs.getString("extra-tags") : "";
    this.spdxExpression = inputs.getString("spdx-expression");
    this.errorOnConflict = inputs.getBool("error-on-conflict");
    this.includeOutputPaths = inputs.getBool("include-output-paths");
    this.flakeHubPushBinary = inputs.getStringOrNull("flakehub-push-binary");
  }
  verifyVisibility() {
    const visibility = inputs.getString("visibility");
    if (!VISIBILITY_OPTIONS.includes(visibility)) {
      throw new Error(
        `Visibility option \`${visibility}\` not recognized. Available options: ${VISIBILITY_OPTIONS.join(", ")}.`
      );
    }
    return visibility;
  }
  async executionEnvironment() {
    const env = {};
    env.FLAKEHUB_PUSH_VISIBLITY = this.visibility;
    env.FLAKEHUB_PUSH_TAG = this.tag;
    env.FLAKEHUB_PUSH_HOST = this.host;
    env.FLAKEHUB_PUSH_LOG_DIRECTIVES = this.logDirectives;
    env.FLAKEHUB_PUSH_LOGGER = this.logger;
    env.FLAKEHUB_PUSH_GITHUB_TOKEN = this.gitHubToken;
    env.FLAKEHUB_PUSH_NAME = this.flakeName;
    env.FLAKEHUB_PUSH_REPOSITORY = this.repository;
    env.FLAKEHUB_PUSH_DIRECTORY = this.directory;
    env.FLAKEHUB_PUSH_GIT_ROOT = this.gitRoot;
    env.FLAKEHUB_PUSH_EXTRA_LABELS = this.extraLabels;
    env.FLAKEHUB_PUSH_SPDX_EXPRESSION = this.spdxExpression;
    if (!this.rolling) {
      env.FLAKEHUB_PUSH_ROLLING = "false";
    }
    if (!this.mirror) {
      env.FLAKEHUB_PUSH_MIRROR = "false";
    }
    if (!this.errorOnConflict) {
      env.FLAKEHUB_PUSH_ERROR_ON_CONFLICT = "false";
    }
    if (!this.includeOutputPaths) {
      env.FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS = "false";
    }
    if (this.rollingMinor !== null) {
      env.FLAKEHUB_PUSH_ROLLING_MINOR = this.rollingMinor.toString();
    }
    return env;
  }
  get flakeName() {
    let name;
    const org = process.env["GITHUB_REPOSITORY_OWNER"];
    const repo = process.env["GITHUB_REPOSITORY"];
    if (this.name !== null) {
      if (this.name === "") {
        throw new Error("The `name` field can't be an empty string");
      }
      const parts = this.name.split("/");
      if (parts.length === 1 || parts.length > 2) {
        throw new Error("The specified `name` must of the form {org}/{repo}");
      }
      const orgName = parts.at(0);
      const repoName = parts.at(1);
      if (orgName !== org) {
        throw new Error(
          `The org name \`${orgName}\` that you specified using the \`name\` input doesn't match the actual org name \`${org}\``
        );
      }
      name = `${orgName}/${repoName}`;
    } else {
      name = `${org}/${repo}`;
    }
    return name;
  }
  async push() {
    const executionEnv = await this.executionEnvironment();
    const binary = this.flakeHubPushBinary !== null ? this.flakeHubPushBinary : await this.idslib.fetchExecutable();
    actionsCore.debug(
      `execution environment: ${JSON.stringify(executionEnv, null, 2)}`
    );
    const exitCode = await actionsExec.exec(binary, [], {
      env: {
        ...executionEnv,
        ...process.env
        // To get PATH, etc.
      }
    });
    if (exitCode !== 0) {
      this.idslib.recordEvent(EVENT_EXECUTION_FAILURE, {
        exitCode
      });
      throw new Error(`non-zero exit code of ${exitCode} detected`);
    }
  }
};
function main() {
  const flakeHubPush = new FlakeHubPushAction();
  flakeHubPush.idslib.onMain(async () => {
    await flakeHubPush.push();
  });
  flakeHubPush.idslib.execute();
}
main();
//# sourceMappingURL=index.js.map
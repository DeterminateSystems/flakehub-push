import * as actionsCore from "@actions/core";
import * as actionsExec from "@actions/exec";
import { ActionOptions, IdsToolbox, inputs } from "detsys-ts";

const EVENT_EXECUTION_FAILURE = "execution_failure";

const VISIBILITY_OPTIONS = ["public", "unlisted", "private"];

type Visibility = "public" | "unlisted" | "private";

type ExecutionEnvironment = {
  FLAKEHUB_PUSH_VISIBILITY?: string;
  FLAKEHUB_PUSH_TAG?: string;
  FLAKEHUB_PUSH_HOST?: string;
  FLAKEHUB_PUSH_LOG_DIRECTIVES?: string;
  FLAKEHUB_PUSH_LOGGER?: string;
  FLAKEHUB_PUSH_GITHUB_TOKEN?: string;
  FLAKEHUB_PUSH_NAME?: string;
  FLAKEHUB_PUSH_REPOSITORY?: string;
  FLAKEHUB_PUSH_DIRECTORY?: string;
  FLAKEHUB_PUSH_GIT_ROOT?: string;
  FLAKEHUB_PUSH_EXTRA_LABELS?: string;
  FLAKEHUB_PUSH_SPDX_EXPRESSION?: string;
  FLAKEHUB_PUSH_ERROR_ON_CONFLICT?: string;
  FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS?: string;
  FLAKEHUB_PUSH_ROLLING?: string;
  FLAKEHUB_PUSH_MIRROR?: string;
  FLAKEHUB_PUSH_ROLLING_MINOR?: string;
};

class FlakeHubPushAction {
  idslib: IdsToolbox;

  // Action inputs translated into environment variables to pass to flakehub-push
  private visibility: Visibility;
  private tag: string;
  private host: string;
  private logDirectives: string;
  private logger: string;
  private gitHubToken: string;
  private name: string | null;
  private repository: string;
  private directory: string;
  private gitRoot: string;
  private extraLabels: string;
  private spdxExpression: string;
  private errorOnConflict: boolean;
  private includeOutputPaths: boolean;
  private rolling: boolean;
  private mirror: boolean;
  private rollingMinor: number | null;

  // Other Action inputs
  private sourceBinary: string | null;

  constructor() {
    const options: ActionOptions = {
      name: "flakehub-push",
      fetchStyle: "gh-env-style",
      diagnosticsUrl: new URL(
        "https://install.determinate.systems/flakehub-push/telemetry",
      ),
      legacySourcePrefix: "flakehub-push",
      requireNix: "fail",
    };

    this.idslib = new IdsToolbox(options);

    // Inputs translated into environment variables for flakehub-push
    this.visibility = this.determineVisibility();
    this.tag = inputs.getString("tag");
    this.host = inputs.getString("host");
    this.logDirectives = inputs.getString("log-directives");
    this.logger = inputs.getString("logger");
    this.gitHubToken = inputs.getString("github-token");
    this.name = inputs.getStringOrNull("name");
    this.repository = inputs.getString("repository");
    this.directory = inputs.getString("directory");
    this.gitRoot = inputs.getString("git-root");
    // extra-tags is deprecated but we still honor it
    this.extraLabels =
      inputs.getString("extra-labels") === ""
        ? inputs.getString("extra-tags")
        : "";
    this.spdxExpression = inputs.getString("spdx-expression");
    this.errorOnConflict = inputs.getBool("error-on-conflict");
    this.includeOutputPaths = inputs.getBool("include-output-paths");
    this.rolling = inputs.getBool("rolling");
    this.mirror = inputs.getBool("mirror");
    this.rollingMinor = inputs.getNumberOrNull("rolling-minor");

    // Other inputs
    this.sourceBinary = this.determineSourceBinary;
  }

  // We first check for a value using `source-binary` and then check the
  // now-deprecated `flakehub-push-binary`
  private get determineSourceBinary(): string | null {
    return (
      inputs.getStringOrNull("source-binary") ??
      inputs.getStringOrNull("flakehub-push-binary")
    );
  }

  private determineVisibility(): Visibility {
    const visibility = inputs.getString("visibility");
    if (!VISIBILITY_OPTIONS.includes(visibility)) {
      actionsCore.setFailed(
        `Visibility option \`${visibility}\` not recognized. Available options: ${VISIBILITY_OPTIONS.map((opt) => `\`${opt}\``).join(", ")}.`,
      );
    }
    return visibility as Visibility;
  }

  private async executionEnvironment(): Promise<ExecutionEnvironment> {
    const env: ExecutionEnvironment = {};

    env.FLAKEHUB_PUSH_VISIBILITY = this.visibility;
    env.FLAKEHUB_PUSH_TAG = this.tag;
    env.FLAKEHUB_PUSH_HOST = this.host;
    env.FLAKEHUB_PUSH_LOG_DIRECTIVES = this.logDirectives;
    env.FLAKEHUB_PUSH_LOGGER = this.logger;
    env.FLAKEHUB_PUSH_GITHUB_TOKEN = this.gitHubToken;
    env.FLAKEHUB_PUSH_NAME = this.flakeName;
    env.FLAKEHUB_PUSH_REPOSITORY = this.repository;
    env.FLAKEHUB_PUSH_DIRECTORY = this.directory;
    env.FLAKEHUB_PUSH_GIT_ROOT = this.gitRoot;
    // not included: the now-deprecated FLAKEHUB_PUSH_EXTRA_TAGS
    env.FLAKEHUB_PUSH_EXTRA_LABELS = this.extraLabels;
    env.FLAKEHUB_PUSH_SPDX_EXPRESSION = this.spdxExpression;
    env.FLAKEHUB_PUSH_ERROR_ON_CONFLICT = this.errorOnConflict.toString();
    env.FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS = this.includeOutputPaths.toString();
    env.FLAKEHUB_PUSH_ROLLING = this.rolling.toString();
    env.FLAKEHUB_PUSH_MIRROR = this.mirror.toString();

    if (this.rollingMinor !== null) {
      env.FLAKEHUB_PUSH_ROLLING_MINOR = this.rollingMinor.toString();
    }

    return env;
  }

  private get flakeName(): string {
    let name: string;

    const githubOrg = process.env["GITHUB_REPOSITORY_OWNER"];
    const githubRepo = process.env["GITHUB_REPOSITORY"];

    if (this.name !== null) {
      if (this.name === "") {
        actionsCore.setFailed("The `name` field can't be an empty string");
      }

      const parts = this.name.split("/");

      if (parts.length === 1 || parts.length > 2) {
        actionsCore.setFailed(
          "The specified `name` must of the form {org}/{repo}",
        );
      }

      const suppliedOrgName = parts.at(0);
      const suppliedRepoName = parts.at(1);

      // Fail on mismatched org names only when *not* mirroring
      if (suppliedOrgName !== githubOrg && !this.mirror) {
        actionsCore.setFailed(
          `The org name \`${suppliedOrgName}\` that you specified using the \`name\` input doesn't match the actual GitHub org name \`${githubOrg}\``,
        );
      }

      name = `${suppliedOrgName}/${suppliedRepoName}`;
    } else {
      name = `${githubOrg}/${githubRepo}`;
    }

    return name;
  }

  async push(): Promise<void> {
    const executionEnv = await this.executionEnvironment();

    const binary =
      this.sourceBinary !== null
        ? this.sourceBinary
        : await this.idslib.fetchExecutable();

    actionsCore.debug(
      `execution environment: ${JSON.stringify(executionEnv, null, 2)}`,
    );

    const exitCode = await actionsExec.exec(
      binary,
      ["--visibility", this.visibility],
      {
        env: {
          ...executionEnv,
          ...process.env, // To get PATH, etc.
        },
      },
    );

    if (exitCode !== 0) {
      this.idslib.recordEvent(EVENT_EXECUTION_FAILURE, {
        exitCode,
      });
      actionsCore.setFailed(`non-zero exit code of ${exitCode} detected`);
    } else {
      actionsCore.info(`Flake release was successfully published`);
    }
  }
}

function main(): void {
  const flakeHubPush = new FlakeHubPushAction();

  flakeHubPush.idslib.onMain(async () => {
    await flakeHubPush.push();
  });

  flakeHubPush.idslib.execute();
}

main();

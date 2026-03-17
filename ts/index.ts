import * as actionsCore from "@actions/core";
import * as actionsExec from "@actions/exec";
import * as actionsGithub from "@actions/github";
import { DetSysAction, inputs } from "detsys-ts";

const EVENT_EXECUTION_FAILURE = "execution_failure";

const FACT_PUSH_ATTEMPT_FROM_PR = "push_attempt_from_pr";

type ExecutionEnvironment = {
  FLAKEHUB_PUSH_VISIBILITY?: string;
  FLAKEHUB_PUSH_TAG?: string;
  FLAKEHUB_PUSH_REV?: string;
  FLAKEHUB_PUSH_HOST?: string;
  FLAKEHUB_PUSH_LOG_DIRECTIVES?: string;
  FLAKEHUB_PUSH_LOGGER?: string;
  FLAKEHUB_PUSH_GITHUB_TOKEN?: string;
  FLAKEHUB_PUSH_NAME?: string;
  FLAKEHUB_PUSH_REPOSITORY?: string;
  FLAKEHUB_PUSH_DIRECTORY?: string;
  FLAKEHUB_PUSH_GIT_ROOT?: string;
  FLAKEHUB_PUSH_MY_FLAKE_IS_TOO_BIG?: string;
  FLAKEHUB_PUSH_EXTRA_LABELS?: string;
  FLAKEHUB_PUSH_SPDX_EXPRESSION?: string;
  FLAKEHUB_PUSH_ERROR_ON_CONFLICT?: string;
  FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS?: string;
  FLAKEHUB_PUSH_ROLLING?: string;
  FLAKEHUB_PUSH_MIRROR?: string;
  FLAKEHUB_PUSH_ROLLING_MINOR?: string;
  GITHUB_CONTEXT?: string;
};

class FlakeHubPushAction extends DetSysAction {
  // Action inputs translated into environment variables to pass to flakehub-push
  private visibility: string;
  private tag: string;
  private rev: string;
  private host: string;
  private logDirectives: string;
  private logger: string;
  private gitHubToken: string;
  private repository: string;
  private directory: string;
  private gitRoot: string;
  private myFlakeIsTooBig: boolean;
  private spdxExpression: string;
  private errorOnConflict: boolean;
  private includeOutputPaths: boolean;
  private rolling: boolean;
  private mirror: boolean;
  private name: string | null;
  private rollingMinor: number | null;

  constructor() {
    super({
      name: "flakehub-push",
      fetchStyle: "gh-env-style",
      diagnosticsSuffix: "diagnostic",
      legacySourcePrefix: "flakehub-push",
      requireNix: "fail",
    });

    // Inputs translated into environment variables for flakehub-push
    this.visibility = inputs.getString("visibility");
    this.tag = inputs.getString("tag");
    this.rev = inputs.getString("rev");
    this.host = inputs.getString("host");
    this.logDirectives = inputs.getString("log-directives");
    this.logger = inputs.getString("logger");
    this.gitHubToken = inputs.getString("github-token");
    this.repository = inputs.getString("repository");
    this.directory = inputs.getString("directory");
    this.gitRoot = inputs.getString("git-root");
    this.myFlakeIsTooBig = inputs.getBool("my-flake-is-too-big");
    this.spdxExpression = inputs.getString("spdx-expression");
    this.errorOnConflict = inputs.getBool("error-on-conflict");
    this.includeOutputPaths = inputs.getBool("include-output-paths");
    this.rolling = inputs.getBool("rolling");
    this.mirror = inputs.getBool("mirror");
    this.name = inputs.getStringOrNull("name");
    this.rollingMinor = inputs.getNumberOrNull("rolling-minor");
  }

  async main(): Promise<void> {
    await this.pushFlakeToFlakeHub();
  }

  // No post step
  async post(): Promise<void> {}

  // extra-tags is deprecated but we still honor it
  private get extraLabels(): string {
    const labels = inputs.getString("extra-labels"); // current input name
    const tags = inputs.getString("extra-tags"); // deprecated input name

    // If `extra-labels` is set to something use it, otherwise use `extra-tags`.
    // It `extra-tags` is also not set, which means that it's an empty string, that's
    // still valid, as the flakehub-push CLI expects a comma-separated list here.
    return labels !== "" ? labels : tags;
  }

  // We first check for a value using the `source-binary` input and fall back to the
  // now-deprecated `flakehub-push-binary`
  private get sourceBinary(): string | null {
    const sourceBinaryInput = inputs.getStringOrNull("source-binary");
    const flakeHubPushBinaryInput = inputs.getStringOrNull(
      "flakehub-push-binary",
    );

    return sourceBinaryInput !== ""
      ? sourceBinaryInput
      : flakeHubPushBinaryInput;
  }

  private executionEnvironment(): ExecutionEnvironment {
    const env: ExecutionEnvironment = {};

    env.FLAKEHUB_PUSH_VISIBILITY = this.visibility;
    env.FLAKEHUB_PUSH_TAG = this.tag;
    env.FLAKEHUB_PUSH_REV = this.rev;
    env.FLAKEHUB_PUSH_HOST = this.host;
    env.FLAKEHUB_PUSH_LOG_DIRECTIVES = this.logDirectives;
    env.FLAKEHUB_PUSH_LOGGER = this.logger;
    env.FLAKEHUB_PUSH_GITHUB_TOKEN = this.gitHubToken;
    env.FLAKEHUB_PUSH_REPOSITORY = this.repository;
    env.FLAKEHUB_PUSH_DIRECTORY = this.directory;
    env.FLAKEHUB_PUSH_GIT_ROOT = this.gitRoot;
    env.FLAKEHUB_PUSH_MY_FLAKE_IS_TOO_BIG = this.myFlakeIsTooBig.toString();
    // not included: the now-deprecated FLAKEHUB_PUSH_EXTRA_TAGS
    env.FLAKEHUB_PUSH_EXTRA_LABELS = this.extraLabels;
    env.FLAKEHUB_PUSH_SPDX_EXPRESSION = this.spdxExpression;
    env.FLAKEHUB_PUSH_ERROR_ON_CONFLICT = this.errorOnConflict.toString();
    env.FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS = this.includeOutputPaths.toString();
    env.FLAKEHUB_PUSH_ROLLING = this.rolling.toString();
    env.FLAKEHUB_PUSH_MIRROR = this.mirror.toString();

    env.GITHUB_CONTEXT = JSON.stringify(actionsGithub.context);

    if (this.name !== null) {
      env.FLAKEHUB_PUSH_NAME = this.name;
    }

    if (this.rollingMinor !== null) {
      env.FLAKEHUB_PUSH_ROLLING_MINOR = this.rollingMinor.toString();
    }

    return env;
  }

  async pushFlakeToFlakeHub(): Promise<void> {
    if (actionsGithub.context.payload.pull_request) {
      actionsCore.setFailed(
        "flakehub-push cannot be triggered from pull requests",
      );
      this.addFact(FACT_PUSH_ATTEMPT_FROM_PR, true);
      return;
    }

    const executionEnv = this.executionEnvironment();

    const flakeHubPushBinary =
      this.sourceBinary !== null
        ? this.sourceBinary
        : await this.fetchExecutable();

    actionsCore.debug(
      `execution environment: ${JSON.stringify(executionEnv, null, 2)}`,
    );

    const exitCode = await actionsExec.exec(flakeHubPushBinary, [], {
      ignoreReturnCode: true,
      env: {
        ...executionEnv,
        ...process.env, // To get PATH, etc.
      },
    });

    if (exitCode !== 0) {
      this.recordEvent(EVENT_EXECUTION_FAILURE, {
        exitCode,
      });
      actionsCore.setFailed(`non-zero exit code of ${exitCode} detected`);
    } else {
      actionsCore.info(`Flake release was successfully published`);
    }
  }
}

function main(): void {
  new FlakeHubPushAction().execute();
}

main();

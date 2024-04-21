import * as actionsCore from "@actions/core";
import * as actionsExec from "@actions/exec";
import { ActionOptions, IdsToolbox, inputs, platform } from "detsys-ts";

const EVENT_EXECUTION_FAILURE = "execution_failure";

const VISIBILITY_OPTIONS = ["public", "unlisted", "private"];

type Visibility = "public" | "unlisted" | "private";

type ExecutionEnvironment = {
  RUST_BACKTRACE?: string;

  FLAKEHUB_PUSH_VISIBLITY?: string;
  FLAKEHUB_PUSH_TAG?: string;
  FLAKEHUB_PUSH_ROLLING_MINOR?: string;
  FLAKEHUB_PUSH_ROLLING?: string;
  FLAKEHUB_PUSH_HOST?: string;
  FLAKEHUB_PUSH_LOG_DIRECTIVES?: string;
  FLAKEHUB_PUSH_LOGGER?: string;
  FLAKEHUB_PUSH_GITHUB_TOKEN?: string;
  FLAKEHUB_PUSH_NAME?: string;
  FLAKEHUB_PUSH_MIRROR?: string;
  FLAKEHUB_PUSH_REPOSITORY?: string;
  FLAKEHUB_PUSH_DIRECTORY?: string;
  FLAKEHUB_PUSH_GIT_ROOT?: string;
  FLAKEHUB_PUSH_EXTRA_LABELS?: string;
  FLAKEHUB_PUSH_EXTRA_TAGS?: string;
  FLAKEHUB_PUSH_SPDX_EXPRESSION?: string;
  FLAKEHUB_PUSH_ERROR_ON_CONFLICT?: string;
  FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS?: string;
};

class FlakeHubPushAction {
  idslib: IdsToolbox;
  private architecture: string;

  // Action inputs
  private visibility: Visibility;
  private name: string | null;
  private repository: string;
  private mirror: boolean;
  private directory: string;
  private gitRoot: string;
  private tag: string;
  private rollingMinor: string | null;
  private rolling: boolean;
  private host: string;
  private logDirectives: string;
  private logger: string;
  private gitHubToken: string;
  private extraLabels: string;
  private spdxExpression: string;
  private errorOnConflict: boolean;
  private includeOutputPaths: boolean;
  private flakeHubPushBinary: string | null;
  private flakeHubPushBranch: string;
  private flakeHubPushPullRequest: string | null;
  private flakeHubPushRevision: string | null;
  private flakeHubPushTag: string | null;
  private flakeHubPushUrl: string | null;

  constructor() {
    const options: ActionOptions = {
      name: "flakehub-push",
      fetchStyle: "gh-env-style",
      diagnosticsUrl: new URL(
        "https://install.determinate.systems/flakehub-push/telemetry",
      ),
      legacySourcePrefix: "flakehub-push",
      requireNix: "ignore",
    };

    this.idslib = new IdsToolbox(options);
    this.architecture = platform.getArchOs();

    // Inputs
    const visibility = this.verifyVisibility();
    this.visibility = visibility;

    this.name = inputs.getStringOrNull("name");
    this.repository = inputs.getString("repository");
    this.mirror = inputs.getBool("mirror");
    this.directory = inputs.getString("directory");
    this.gitRoot = inputs.getString("git-root");
    this.tag = inputs.getString("tag");
    this.rollingMinor = inputs.getStringOrNull("rolling-minor");
    this.rolling = inputs.getBool("rolling");
    this.host = inputs.getString("host");
    this.logDirectives = inputs.getString("log-directives");
    this.logger = inputs.getString("logger");
    this.gitHubToken = inputs.getString("github-token");
    // extra-tags is deprecated but we still honor it
    this.extraLabels =
      inputs.getString("extra-labels") === ""
        ? inputs.getString("extra-tags")
        : "";
    this.spdxExpression = inputs.getString("spdx-expression");
    this.errorOnConflict = inputs.getBool("error-on-conflict");
    this.includeOutputPaths = inputs.getBool("include-output-paths");
    this.flakeHubPushBinary = inputs.getStringOrNull("flakehub-push-binary");
    this.flakeHubPushBranch = inputs.getString("flakehub-push-branch");
    this.flakeHubPushPullRequest = inputs.getStringOrNull("flakehub-push-pr");
    this.flakeHubPushRevision = inputs.getStringOrNull(
      "flakehub-push-revision",
    );
    this.flakeHubPushTag = inputs.getStringOrNull("flakehub-push-tag");
    this.flakeHubPushUrl = inputs.getStringOrNull("flakehub-push-url");
  }

  private verifyVisibility(): Visibility {
    const visibility = inputs.getString("visibility");
    if (!VISIBILITY_OPTIONS.includes(visibility)) {
      actionsCore.setFailed(
        `Visibility option \`${visibility}\` not recognized. Available options: ${VISIBILITY_OPTIONS.join(", ")}.`,
      );
    }
    return visibility as Visibility;
  }

  private makeUrl(endpoint: string, item: string): string {
    return `https://install.determinate.systems/${this.name}/${endpoint}/${item}/${this.architecture}?ci=github`;
  }

  private get defaultBinaryUrl(): string {
    return `https://install.determinate.systems/${this.name}/stable/${this.architecture}?ci=github`;
  }

  private async executionEnvironment(): Promise<ExecutionEnvironment> {
    const env: ExecutionEnvironment = {};

    env.FLAKEHUB_PUSH_VISIBLITY = this.visibility;
    env.FLAKEHUB_PUSH_ROLLING = this.rolling.toString();
    env.FLAKEHUB_PUSH_HOST = this.host;
    env.FLAKEHUB_PUSH_LOG_DIRECTIVES = this.logDirectives;
    env.FLAKEHUB_PUSH_LOGGER = this.logger;
    env.FLAKEHUB_PUSH_GITHUB_TOKEN = this.gitHubToken;
    env.FLAKEHUB_PUSH_NAME = this.flakeName;
    env.FLAKEHUB_PUSH_MIRROR = this.mirror.toString();
    env.FLAKEHUB_PUSH_REPOSITORY = this.repository;
    env.FLAKEHUB_PUSH_DIRECTORY = this.directory;
    env.FLAKEHUB_PUSH_GIT_ROOT = this.gitRoot;
    env.FLAKEHUB_PUSH_EXTRA_LABELS = this.extraLabels;
    env.FLAKEHUB_PUSH_SPDX_EXPRESSION = this.spdxExpression;
    env.FLAKEHUB_PUSH_ERROR_ON_CONFLICT = this.errorOnConflict.toString();
    env.FLAKEHUB_PUSH_INCLUDE_OUTPUT_PATHS = this.includeOutputPaths.toString();

    if (this.flakeHubPushTag !== null) {
      env.FLAKEHUB_PUSH_TAG = this.flakeHubPushTag;
    }

    if (this.rollingMinor !== null) {
      env.FLAKEHUB_PUSH_ROLLING_MINOR = this.rollingMinor;
    }

    return env;
  }

  private get flakeName(): string {
    let name: string;

    const org = process.env["GITHUB_REPOSITORY_OWNER"];
    const repo = process.env["GITHUB_REPOSITORY"];

    if (this.name !== null) {
      if (this.name === "") {
        actionsCore.setFailed("The `name` field can't be an empty string");
      }

      const parts = this.name.split("/");

      if (parts.length === 1 || parts.length > 2) {
        actionsCore.setFailed(
          "The specified `name` must of the form {org}/{flake}",
        );
      }

      if (parts.at(0) !== org) {
        actionsCore.setFailed(
          `The org name \`${parts.at(0)}\` that you specified using the \`name\` input doesn't match the actual org name \`${org}\``,
        );
      }

      name = `${parts.at(0)}/${parts.at(1)}`;
    } else {
      name = `${org}/${repo}`;
    }

    return name;
  }

  async push(): Promise<void> {
    const executionEnv = await this.executionEnvironment();

    const binary =
      this.flakeHubPushBinary !== null
        ? this.flakeHubPushBinary
        : await this.idslib.fetchExecutable();

    actionsCore.debug(
      `execution environment: ${JSON.stringify(executionEnv, null, 2)}`,
    );

    const exitCode = await actionsExec.exec(binary, [], {
      env: {
        ...executionEnv,
        ...process.env, // To get PATH, etc.
      },
    });

    if (exitCode !== 0) {
      this.idslib.recordEvent(EVENT_EXECUTION_FAILURE, {
        exitCode,
      });
      actionsCore.setFailed(`non-zero exit code of ${exitCode} detected`);
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

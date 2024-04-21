import * as actionsCore from "@actions/core";
import { ActionOptions, IdsToolbox, inputs, platform } from "detsys-ts";

type Visibility = "public" | "unlisted" | "private";

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
      requireNix: "ignore",
    };

    this.idslib = new IdsToolbox(options);
    this.architecture = platform.getArchOs();

    // Inputs
    // TODO: check enum values
    const visibility = inputs.getString("visibility") as Visibility;
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

  private makeUrl(endpoint: string, item: string): string {
    return `https://install.determinate.systems/flakehub-push/${endpoint}/${item}/${this.architecture}?ci=github`;
  }

  private get defaultBinaryUrl(): string {
    return `https://install.determinate.systems/flakehub-push/stable/${this.architecture}?ci=github`;
  }

  private get pushBinaryUrl(): string {
    if (this.flakeHubPushBinary !== null) {
      return this.flakeHubPushBinary;
    } else if (this.flakeHubPushPullRequest !== null) {
      return this.makeUrl("pr", this.flakeHubPushPullRequest);
    } else if (this.flakeHubPushTag !== null) {
      return this.makeUrl("tag", this.flakeHubPushTag);
    } else if (this.flakeHubPushRevision !== null) {
      return this.makeUrl("rev", this.flakeHubPushRevision);
    } else if (this.flakeHubPushBranch !== null) {
      return this.makeUrl("branch", this.flakeHubPushBranch);
    } else {
      return this.defaultBinaryUrl;
    }
  }

  async push(): Promise<void> {
    const pusbBinaryUrl = this.pushBinaryUrl;
    actionsCore.info(`Fetching flakehub-push binary from ${pusbBinaryUrl}`);
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

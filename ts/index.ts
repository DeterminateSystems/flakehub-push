import * as actionsCore from "@actions/core";
import { ActionOptions, IdsToolbox, inputs } from "detsys-ts";

type Visibility = "public" | "unlisted" | "private";

class FlakeHubPushAction {
  idslib: IdsToolbox;
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
  private githubToken: string;
  private extraLabels: string;
  private spdxExpression: string;
  private errorOnConflict: boolean;
  private includeOutputPaths: boolean;
  private flakehubPushBinary: string;
  private flakehubPushBranch: string;
  private flakehubPushPullRequest: string;
  private flakehubPushRevision: string;
  private flakehubPushTag: string;
  private flakehubPushUrl: string;

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
    this.githubToken = inputs.getString("github-token");
    // extra-tags is deprecated but we still honor it
    this.extraLabels =
      inputs.getString("extra-labels") === ""
        ? inputs.getString("extra-tags")
        : "";
    this.spdxExpression = inputs.getString("spdx-expression");
    this.errorOnConflict = inputs.getBool("error-on-conflict");
    this.includeOutputPaths = inputs.getBool("include-output-paths");
    this.flakehubPushBinary = inputs.getString("flakehub-push-binary");
    this.flakehubPushBranch = inputs.getString("flakehub-push-branch");
    this.flakehubPushPullRequest = inputs.getString("flakehub-push-pr");
    this.flakehubPushRevision = inputs.getString("flakehub-push-revision");
    this.flakehubPushTag = inputs.getString("flakehub-push-tag");
    this.flakehubPushUrl = inputs.getString("flakehub-push-url");
  }

  async push(): Promise<void> {
    actionsCore.info("Done");
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

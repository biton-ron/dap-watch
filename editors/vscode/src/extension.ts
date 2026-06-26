import * as vscode from "vscode";

export function activate(context: vscode.ExtensionContext) {
  const factory = new DapWatchAdapterFactory();

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("dap-watch", factory)
  );
}

class DapWatchAdapterFactory
  implements vscode.DebugAdapterDescriptorFactory
{
  createDebugAdapterDescriptor(
    _session: vscode.DebugSession
  ): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
    return new vscode.DebugAdapterExecutable(
      "cargo",
      ["run", "--quiet", "--bin", "dap-watch", "--", "--stdio"],
      { cwd: "/Users/ronbiton/projects/rust-projects/dap-watch" }
    );
  }
}

export function deactivate() {}

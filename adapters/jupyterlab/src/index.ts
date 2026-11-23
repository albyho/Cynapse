import type {
  JupyterFrontEnd,
  JupyterFrontEndPlugin,
} from '@jupyterlab/application'
import { Notification } from '@jupyterlab/apputils'
import { IDocumentManager } from '@jupyterlab/docmanager'
import { IFileBrowserFactory } from '@jupyterlab/filebrowser'
import { INotebookTracker } from '@jupyterlab/notebook'
import { ServerConnection } from '@jupyterlab/services'

import {
  JupyterLabAdapterConnection,
  type JupyterLabAdapterStatus,
} from './adapterConnection'
import { createJupyterLabActionHandler } from './actions'
import { createJupyterLabCheckHandler } from './check'
import { JupyterLabNotebookFacade } from './jupyterLabFacade'
import {
  createJupyterServerTerminalHost,
  JupyterServerTerminalFacade,
} from './terminalFacade'
import { createWorkspaceResetHandler } from './workspaceReset'

const plugin: JupyterFrontEndPlugin<void> = {
  id: 'cynapse-jupyterlab-adapter:plugin',
  autoStart: true,
  requires: [INotebookTracker, IDocumentManager, IFileBrowserFactory],
  activate: (
    app: JupyterFrontEnd,
    tracker: INotebookTracker,
    documents: IDocumentManager,
    fileBrowserFactory: IFileBrowserFactory,
  ) => {
    const settings = app.serviceManager.serverSettings
    const terminal = new JupyterServerTerminalFacade(
      createJupyterServerTerminalHost({
        baseUrl: settings.baseUrl,
        fetch: ((input: RequestInfo | URL, init?: RequestInit) =>
          ServerConnection.makeRequest(
            String(input),
            init ?? {},
            settings,
          )) as typeof fetch,
        createWebSocket: (url) => new WebSocket(withToken(url, settings.token)),
      }),
    )
    const notebooks = new JupyterLabNotebookFacade(
      app,
      tracker,
      documents,
      app.serviceManager,
      fileBrowserFactory,
    )

    const connection = new JupyterLabAdapterConnection(
      {},
      reportStatus,
      createJupyterLabActionHandler(notebooks, terminal),
      createJupyterLabCheckHandler(notebooks, terminal),
      createWorkspaceResetHandler({
        openNotebookPaths: () => notebooks.openNotebookPaths(),
        closeNotebook: (path, signal) => notebooks.closeNotebook(path, signal),
        shutdownKernel: (path, signal) =>
          notebooks.shutdownKernel(path, signal),
        closeTerminals: (paths, excludePaths, sessionKeys) =>
          terminal.disposeUnderPaths(paths, excludePaths, sessionKeys),
      }),
    )

    // Actions must not race the workspace restore that creates the Notebook
    // tracker and document manager state.
    void app.restored.then(() => connection.connect())
  },
}

function reportStatus(status: JupyterLabAdapterStatus): void {
  console.log('[cynapse]', status)
  if (status.kind === 'inactive') {
    Notification.warning(status.message, { autoClose: false })
  } else if (status.kind === 'error') {
    Notification.error(status.message, { autoClose: false })
  }
}

/** The terminal WebSocket authenticates with the same token as the page. */
function withToken(url: string, token: string): string {
  if (token.length === 0) {
    return url
  }
  const target = new URL(url)
  target.searchParams.set('token', token)
  return target.toString()
}

export default plugin

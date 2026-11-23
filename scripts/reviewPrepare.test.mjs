import assert from 'node:assert/strict'
import test from 'node:test'

import { reviewTriggerPaths } from './reviewPrepare.mjs'

test('review triggers cover cross-component paths and skip app-only UI and tests', () => {
  const files = [
    'adapters/adapter-kit/src/connection.ts',
    'adapters/jupyterlab/src/protocol.ts',
    'adapters/vscode/README.md',
    'client/app/src/main.rs',
    'client/app/src/settings.rs',
    'client/app/src/platform/window.rs',
    'client/app/src/view_model.rs',
    'client/app/src/views/main.rs',
    'client/core/src/server/connection.rs',
    'client/core/src/course_session/tests.rs',
    'client/core/src/course_session/tests/playback.rs',
    'client/harness/src/runner.rs',
    'client/model/src/session.rs',
    'client/model/src/session/tests/jupyterlab.rs',
    'course/src/steps.rs',
    'docs/audits/audit.md',
    'protocol/schema.json',
    'spec.md',
  ]

  assert.deepEqual(reviewTriggerPaths(files), [
    'adapters/adapter-kit/src/connection.ts',
    'adapters/jupyterlab/src/protocol.ts',
    'client/app/src/main.rs',
    'client/app/src/settings.rs',
    'client/app/src/platform/window.rs',
    'client/core/src/server/connection.rs',
    'client/harness/src/runner.rs',
    'client/model/src/session.rs',
    'course/src/steps.rs',
    'protocol/schema.json',
    'spec.md',
  ])
})

<script setup lang="ts">
// The console's root: the one place that knows where data comes from, and the one place that answers
// the components' path question.
//
// Every component under `src/components/` takes what it renders as a prop. That is not an accident of
// how they were written — it is the property that let them be lifted out of the flux-connectors docs
// site without a rewrite, and `test/components.test.mjs` holds it. So everything is fetched *here*
// and passed down, and no component under `src/components/` knows a network exists.
//
// Each thing this file reads has three states and each has its own view, because collapsing any two
// of them lies to the reader:
//
//   loading  the request is in flight — say so, and name what is being read
//   failed   nothing was read — name the endpoint that did not answer
//   ready    an answer, however small, including one with nothing in it
//
// The third and second are the pair that matters. A service that is not running and a service with
// nothing in it must never render the same page.
//
// **What surrounds all of that is `ConsoleShell.mts`**, and it is the reason X-34 exists. This
// console renders the connector catalogue, which is reference material — what this build *could*
// run — and while that was the whole surface, the console read as a connector browser rather than
// as the admin surface of a service that holds credentials and executes for many callers. The shell
// states the platform's surfaces instead, including the three that are not built, and `surfaces.mts`
// is the single statement of which is which.

import { computed, onMounted, provide, shallowRef, watch } from 'vue'
import { PATH_RESOLVER } from './catalog.mts'
import { fragmentPath, useRoute } from './routing'
import {
  CONNECTORS_ENDPOINT,
  SIGNIN_ENDPOINT,
  loadCatalogue,
  loadConnections,
  loadSession,
  signOut,
  type CatalogueState,
  type ConnectionsState,
  type SessionState,
} from './service.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { surfaceOfRoute } from './surfaces.mts'
import { isDark, toggleTheme } from './theme'

import AgentOnboarding from './AgentOnboarding.mts'
import CatalogueFailure from './CatalogueFailure.mts'
import Connections from './Connections.mts'
import ConsoleShell from './ConsoleShell.mts'
import OperationFacts from './OperationFacts.mts'
import CatalogExplorer from './components/CatalogExplorer.vue'
import CatalogSnapshot from './components/CatalogSnapshot.vue'
import OperationDetail from './components/OperationDetail.vue'

// The components' one port. Their default is identity, which would be wrong here: this console is a
// single static document, so `/operations/<id>` as a bare href would 404. See `src/routing.ts`.
provide(PATH_RESOLVER, fragmentPath)

// `shallowRef` rather than `ref`: each of these is replaced wholesale and never edited, so making
// every operation in a catalogue deeply reactive would buy nothing and cost a walk of the document.
const catalogue = shallowRef<CatalogueState>({ status: 'loading' })
const session = shallowRef<SessionState>({ status: 'loading' })
const connections = shallowRef<ConnectionsState>({ status: 'loading' })

const route = useRoute()

/** Whether a principal is resolved. `null` while the session is still unknown either way. */
const principal = computed(() => (session.value.status === 'ready' ? session.value.principal : null))
const signedIn = computed(() => principal.value !== null)

onMounted(async () => {
  // Both at once. The catalogue is anonymous and the session is not, so neither waits on the other
  // — and a slow identity provider must not delay the one view that never needed a principal.
  void loadCatalogue().then((state) => (catalogue.value = state))
  session.value = await loadSession()
})

// Connections are tenant data and `/api/connections` requires a principal, so this is not asked
// until one resolves. That is what keeps `ConnectionsState` at three states: a signed-out reader is
// never shown a listing that failed, because none was attempted — the gate below is shown instead.
watch(signedIn, async (resolved) => {
  if (resolved) connections.value = await loadConnections()
})

/** Sign out, then reload — the session cookie is gone and every view on the page depends on it. */
async function endSession() {
  await signOut()
  window.location.reload()
}

// Narrowed once here rather than in the template, so the type checker can see it inside `v-if`.
const ready = computed(() => (catalogue.value.status === 'ready' ? catalogue.value : null))
const failure = computed(() =>
  catalogue.value.status === 'failed' ? catalogue.value.failure : null
)
const sessionFailure = computed(() =>
  session.value.status === 'failed' ? session.value.failure : null
)

/** The served facts about the operation on screen, when the route names one this catalogue has. */
const facts = computed(() =>
  ready.value && route.value.name === 'operation' ? (ready.value.served[route.value.id] ?? null) : null
)

/** Which surface of the platform the reader is on, so the rail can say so. */
const active = computed(() => surfaceOfRoute(route.value.name))
</script>

<template>
  <div class="console">
    <ConsoleShell :session="session" :active="active" @sign-out="endSession">
      <template #theme>
        <button type="button" :aria-pressed="isDark" @click="toggleTheme()">
          {{ isDark ? 'Light' : 'Dark' }}
        </button>
      </template>
    </ConsoleShell>

    <main>
      <!--
        How to connect an agent, and the head of this chain on purpose.

        `docs/vision.md` calls the agent this platform's primary caller, and this is the one screen
        that depends on nothing: no session, no catalogue, no principal. Putting it first says so
        structurally — a branch at the head of the chain has no predecessor that could gate it — and
        `the_page_is_reachable_without_a_session` asserts it stays there. An agent that must
        authenticate to learn how to authenticate is a closed loop.

        It is passed nothing, because it describes the shape of this service and never its contents.
      -->
      <template v-if="route.name === 'connect'">
        <AgentOnboarding />
      </template>

      <!--
        Connections — the first of the console's two jobs, and where a reader lands.

        Behind a principal, because a connection is tenant data. The gate is not an empty state: it
        says why there is nothing here and offers the one thing that changes it. `/api/signin`
        answers `303` to the identity provider, so it is an anchor the browser navigates and never
        a fetch.
      -->
      <template v-else-if="route.name === 'connections'">
        <p v-if="session.status === 'loading'" class="console__loading">Reading your session…</p>

        <Connections v-else-if="signedIn" :state="connections" />

        <section v-else class="gate">
          <h1>Sign in to see this tenant's connections</h1>
          <p>
            A connection belongs to a tenant, and the tenant is read from whoever this service
            resolves you to be — never from anything this page could ask for. So there is nothing
            here to show until you sign in.
          </p>
          <p v-if="sessionFailure">
            <code>{{ sessionFailure.endpoint }}</code> did not answer, so this console cannot tell
            whether you are signed in. It is not saying that you are not.
          </p>
          <p><a class="shell__signin" :href="SIGNIN_ENDPOINT">Sign in</a></p>
        </section>
      </template>

      <!-- Everything below is the catalogue: reference material, and no longer the front door. -->

      <!-- Named, so a request that never comes back is visibly a request and not a blank page. -->
      <p v-else-if="catalogue.status === 'loading'" class="console__loading">
        Reading the catalogue from <code>{{ CONNECTORS_ENDPOINT }}</code
        >…
      </p>

      <CatalogueFailure v-else-if="failure" :failure="failure" />

      <template v-else-if="ready">
        <template v-if="route.name === 'explorer'">
          <h1>Catalogue</h1>
          <CatalogSnapshot :catalog="ready.catalog" />
          <CatalogExplorer :catalog="ready.catalog" />
        </template>

        <template v-else-if="route.name === 'operation'">
          <h1><code>{{ route.id }}</code></h1>
          <OperationDetail :catalog="ready.catalog" :id="route.id" />
          <OperationFacts v-if="facts" :operation="facts" />
        </template>

        <!--
          The served catalogue carries no Flux core entries — `core` is `null` — so a `/core/…` link
          resolves to a statement that this source publishes none, rather than to a blank page or to
          the explorer. A link that no longer resolves should say so.
        -->
        <template v-else-if="route.name === 'core'">
          <h1><code>{{ route.entry }}</code></h1>
          <p>
            The flux-exchange catalogue publishes connectors and their operations. It publishes no
            Flux core entries, so nothing here answers to
            <code>/core/{{ route.kind }}/{{ route.entry }}</code
            >. <a :href="fragmentPath('/explorer')">Back to the catalogue</a>.
          </p>
        </template>

        <template v-else>
          <h1>Not found</h1>
          <p>
            Nothing in this console answers to <code>{{ route.path }}</code
            >. <a :href="fragmentPath('/explorer')">Back to the catalogue</a>.
          </p>
        </template>
      </template>
    </main>

    <!--
      The footer, and not the rail. Connecting an agent is a reference an agent author reaches for
      once; the rail is where an operator works, and an entry there would imply it is a place to do
      work. It is first in the line because it is the only one of the three that is a destination.
    -->
    <footer class="console__foot">
      <p>
        <a :href="fragmentPath(ONBOARDING_PATH)">Connect an agent</a> · Catalogue read from
        <code>{{ CONNECTORS_ENDPOINT }}</code> ·
        <a href="https://github.com/codewandler/flux-exchange">codewandler/flux-exchange</a>
      </p>
    </footer>
  </div>
</template>

<style scoped>
.console {
  max-width: 1104px;
  margin: 0 auto;
  padding: 0 24px 64px;
}

.console__loading {
  margin: 32px 0;
  color: var(--vp-c-text-2);
}

.console__foot {
  margin-top: 48px;
  padding-top: 16px;
  border-top: 1px solid var(--vp-c-divider);
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.console__foot code {
  color: inherit;
}
</style>

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
  connect,
  loadCatalogue,
  loadConnections,
  loadDeclaration,
  loadGrants,
  loadSession,
  previewGrant,
  replaceGrants,
  signOut,
  type CatalogueState,
  type ConnectionsState,
  type ConnectOutcome,
  type DeclarationState,
  type GrantOutcome,
  type GrantsState,
  type PreviewState,
  type ProposedGrant,
  type SessionState,
} from './service.mts'
import { ONBOARDING_PATH } from './onboarding.mts'
import { AGENTS_PATH } from './minting.mts'
import { mayGrant } from './granting.mts'
import { surfaceOfRoute } from './surfaces.mts'
import { isDark, toggleTheme } from './theme'

import AgentOnboarding from './AgentOnboarding.mts'
import Agents from './Agents.mts'
import CatalogueFailure from './CatalogueFailure.mts'
import Connect from './Connect.mts'
import Connections from './Connections.mts'
import ConsoleShell from './ConsoleShell.mts'
import Grants from './Grants.mts'
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

// ---------------------------------------------------------------------------------------------
// Wiring a connector up. The console's other job, and until X-44 the one it could not do.
//
// Three pieces of state and no fourth: which connector is being connected, what the service says it
// declares, and what the last attempt did. **What the operator typed is not among them.** A value
// lives in the input element it was typed into and in the request body, and this component holds
// none of it — see `Connect.mts`, which is where that is enforced rather than merely intended.
// ---------------------------------------------------------------------------------------------

const chosen = shallowRef<string | null>(null)
const declaration = shallowRef<DeclarationState | null>(null)
const outcome = shallowRef<ConnectOutcome | null>(null)
const connecting = shallowRef(false)

/** A connector was chosen: ask the service what it declares, and forget the last attempt. */
async function chooseConnector(connector: string) {
  const id = connector || null
  chosen.value = id
  outcome.value = null

  if (id === null) {
    declaration.value = null
    return
  }

  declaration.value = { status: 'loading' }
  const state = await loadDeclaration(id)
  // The reader may have moved on while that was in flight; a declaration for a connector nobody is
  // looking at any more would render under the wrong heading.
  if (chosen.value === id) declaration.value = state
}

/** Connect, then re-read the listing — which is where the result is shown, as addresses. */
async function connectChosen(values: Record<string, string>) {
  const connector = chosen.value
  if (connector === null || connecting.value) return

  connecting.value = true
  outcome.value = await connect(connector, values)
  connecting.value = false

  if (outcome.value.status === 'connected') connections.value = await loadConnections()
}

// ---------------------------------------------------------------------------------------------
// Grants: what this tenant may run. The half of X-62 the service already had.
//
// Read here and passed down, which is this file's ordinary arrangement — and deliberately *not*
// `Agents.mts`'s exception. That one exists because a minted token must not reach this component,
// which is the root and outlives every screen. A grant is a policy: it carries no secret, it is
// meant to be read back, and holding it here costs nothing.
//
// Four pieces of state. The draft is **not** among them: it lives in `Grants.mts`, which emits a
// proposal when it changes and a whole set when it is saved.
// ---------------------------------------------------------------------------------------------

const grants = shallowRef<GrantsState>({ status: 'loading' })
const preview = shallowRef<PreviewState | null>(null)
const grantOutcome = shallowRef<GrantOutcome | null>(null)
const granting = shallowRef(false)

/**
 * Which question the preview panel is currently waiting on.
 *
 * Every edit asks a new one, and answers can arrive out of order — an operator who widens a bound
 * and then narrows it again must not be shown the wider answer because it was slower. Only the
 * newest question's answer is kept, which is `chooseConnector`'s rule made explicit because here
 * the question changes on every keystroke-equivalent rather than once per connector.
 */
let asked = 0

// The route requires a `User` on the read as well as the write, so this is not asked of a caller
// this host would refuse: a `403` rendered as a failed read would tell an agent's operator that the
// service is broken. `Grants.mts` says why there is no listing instead.
watch(
  () => mayGrant(principal.value),
  async (may) => {
    if (may) grants.value = await loadGrants()
  }
)

/** Ask what a proposed grant would admit. Reads the preview route; writes nothing. */
async function previewProposed(proposed: ProposedGrant) {
  const question = ++asked
  preview.value = { status: 'loading' }
  const answer = await previewGrant(proposed)
  if (question === asked) preview.value = answer
}

/** Replace the whole set, then re-read — what is shown is the service's answer, not what was sent. */
async function saveGrants(next: ProposedGrant[]) {
  if (granting.value) return

  granting.value = true
  grantOutcome.value = await replaceGrants(next)
  granting.value = false

  const outcome = grantOutcome.value
  if (outcome.status === 'saved') grants.value = { status: 'ready', grants: outcome.grants, editable: outcome.editable }
}

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

/**
 * Every connector the catalogue lists, by id.
 *
 * The connect form offers these and no others, and the console enumerates none of its own — the
 * same rule the credential inputs follow one level down. Empty while the catalogue is still being
 * read, which leaves the form's chooser empty rather than short: a partial list of connectors would
 * read as the complete one.
 */
const connectors = computed(() => ready.value?.catalog.providers.map((provider) => provider.id) ?? [])

/**
 * Every risk level and every effect the catalogue actually publishes.
 *
 * Handed to the grants screen so that a vocabulary it does not offer as a bound is something it can
 * *say*, rather than something that silently narrows the widest grant an operator can write. The
 * console keeps the ordered list because `max_risk` means "at or below" and an order cannot be
 * recovered from a set of strings; this is what keeps that list from going stale in silence.
 */
const catalogueRisks = computed(() => [
  ...new Set(Object.values(ready.value?.served ?? {}).map((operation) => operation.risk)),
])
const catalogueEffects = computed(() => [
  ...new Set(Object.values(ready.value?.served ?? {}).flatMap((operation) => operation.effects)),
])

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
        Where an operator mints one, and the one screen in this console that renders a credential
        value.

        It is passed the session and nothing else, and it reads `/api/agents` for itself rather than
        being handed a result — deliberately, and against this file's usual arrangement. This
        component is the root, so it outlives every screen; anything it were handed would still be
        in memory after the reader had navigated away, and what the mint answers with is the one
        value on this host that cannot be shown a second time. Holding it here would make that a
        claim about `App.vue` instead of a property of the view. `Agents.mts` sets out the whole of
        it.
      -->
      <template v-else-if="route.name === 'agents'">
        <Agents :session="session" />
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

        <!--
          The listing first, then the form. A reader lands on what is wired up — X-34's decision,
          unchanged — and the thing that changes it sits directly beneath, where the result of using
          it appears. On a successful connect the listing above is re-read, so the addresses are the
          service's answer rather than this page's memory of what was sent.
        -->
        <template v-else-if="signedIn">
          <Connections :state="connections" />
          <Connect
            :connectors="connectors"
            :chosen="chosen"
            :declaration="declaration"
            :outcome="outcome"
            :busy="connecting"
            @choose="chooseConnector"
            @submit="connectChosen"
          />
        </template>

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

      <!--
        Grants — what this tenant may run, and the screen X-13's gate had been waiting for.

        Directly after connections in the rail because the two are one job in two steps: a tenant
        with a connection and no grant runs nothing at all. The screen is handed the session and
        renders its own gate from it — the route admits a `User` on the read as well as the write,
        so an agent's operator is told why rather than being shown a listing that failed.

        The connector list is the catalogue's, so this console enumerates none of its own; the risk
        and effect vocabularies are passed too, so a level the catalogue publishes and the screen
        cannot offer is something it states rather than something it silently drops.
      -->
      <template v-else-if="route.name === 'grants'">
        <Grants
          :session="session"
          :grants="grants"
          :connectors="connectors"
          :catalogue-risks="catalogueRisks"
          :catalogue-effects="catalogueEffects"
          :preview="preview"
          :outcome="grantOutcome"
          :busy="granting"
          @preview="previewProposed"
          @save="saveGrants"
        />
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
      once, and minting one is something an operator does with the identity they already have; the
      rail states what this platform *is*, and an entry there would claim a seventh surface. The two
      destinations come first, and in that order: the reference explains what an agent is for, and
      the screen beside it is where one is created.
    -->
    <footer class="console__foot">
      <p>
        <a :href="fragmentPath(ONBOARDING_PATH)">Connect an agent</a> ·
        <a :href="fragmentPath(AGENTS_PATH)">Mint an agent</a> · Catalogue read from
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

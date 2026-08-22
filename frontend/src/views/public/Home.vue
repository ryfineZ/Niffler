<template>
  <div
    ref="homeRoot"
    class="home-scroll-root relative min-h-screen overflow-x-hidden bg-background text-foreground literary-grid literary-paper lg:h-[calc(100dvh-4rem)] lg:overflow-y-auto lg:snap-y lg:snap-mandatory lg:scroll-smooth"
    :class="`direction-${scrollDirection}`"
    :data-active-scene="activeScene"
    :data-previous-scene="sceneIds[previousSceneIndex]"
  >
    <HomeCinematicVisual
      :scene="activeScene"
      :direction="scrollDirection"
      :reduced-motion="reducedMotion"
    />

    <nav
      class="scene-indicator"
      :aria-label="t('nav.home')"
    >
      <button
        v-for="(label, index) in sceneLabels"
        :key="sceneIds[index]"
        type="button"
        class="scene-indicator-button group"
        :aria-label="label"
        :aria-current="activeSceneIndex === index ? 'step' : undefined"
        @click="scrollToScene(index)"
      >
        <span class="scene-indicator-label">{{ label }}</span>
        <span
          class="scene-indicator-dot"
          :class="{ active: activeSceneIndex === index }"
        />
      </button>
    </nav>

    <main class="relative z-10">
      <section
        :ref="el => setSectionRef(el, 0)"
        class="home-scene home-scene-hero mx-auto max-w-[1480px] px-4 pb-8 pt-8 sm:px-6 sm:pt-12 lg:grid lg:min-h-[calc(100dvh-4rem)] lg:content-center lg:px-8 lg:pb-12 lg:pt-16 lg:snap-start"
        :class="{ 'scene-active': activeSceneIndex === 0 }"
      >
        <div class="hero-shell relative overflow-hidden border border-border/80 bg-background/80 shadow-sm backdrop-blur-sm">
          <div class="hero-glow pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-primary/10 blur-3xl" />
          <div class="relative grid lg:grid-cols-[minmax(0,1.08fr)_minmax(360px,0.92fr)]">
            <div class="hero-copy-panel border-b border-border/70 p-7 sm:p-10 lg:border-b-0 lg:border-r lg:p-16">
              <div class="flex items-center gap-3 text-[11px] font-bold uppercase tracking-[0.2em] text-primary">
                <span class="h-px w-8 bg-primary" />
                {{ t('home.heroEyebrow') }}
              </div>
              <h1 class="mt-7 max-w-3xl font-serif text-5xl font-semibold leading-[0.96] tracking-[-0.05em] sm:text-7xl lg:text-[5.35rem]">
                {{ t('home.heroTitleLine1') }}<br>
                <span class="text-primary">{{ t('home.heroTitleLine2') }}</span>
              </h1>
              <p class="mt-7 max-w-xl text-base leading-8 text-muted-foreground sm:text-lg">
                {{ t('home.heroDescription') }}
              </p>
              <div class="mt-9 flex flex-col gap-3 sm:flex-row">
                <button
                  class="inline-flex h-12 items-center justify-center gap-2 bg-primary px-6 text-sm font-semibold text-primary-foreground transition hover:-translate-y-0.5 hover:bg-primary/90"
                  @click="openPrimaryAction"
                >
                  {{ authStore.isAuthenticated ? t('nav.dashboard') : t('home.startNow') }}
                  <ArrowRight class="h-4 w-4" />
                </button>
                <RouterLink
                  to="/models"
                  class="inline-flex h-12 items-center justify-center gap-2 border border-border bg-background/70 px-6 text-sm font-semibold transition hover:border-primary/50 hover:text-primary"
                >
                  {{ t('home.exploreModels') }}
                  <ArrowUpRight class="h-4 w-4" />
                </RouterLink>
              </div>
              <div class="mt-9 flex flex-wrap gap-x-5 gap-y-2 text-xs text-muted-foreground">
                <span
                  v-for="proof in heroProofs"
                  :key="proof"
                  class="flex items-center gap-2"
                >
                  <CheckCircle2 class="h-4 w-4 text-emerald-500" />{{ proof }}
                </span>
              </div>
              <PublicEndpointLatency
                v-if="showPublicEndpointLatency"
                :base-domain="defaultPortalBaseDomain"
              />
            </div>

            <div class="hero-visual relative flex min-h-[360px] flex-col justify-between overflow-hidden bg-[#26231f] px-7 py-5 text-[#f7f3ea] sm:px-10 sm:py-6 lg:px-12 lg:py-7">
              <div class="hero-grid absolute inset-0 opacity-40 [background-image:linear-gradient(rgba(247,243,234,0.08)_1px,transparent_1px),linear-gradient(90deg,rgba(247,243,234,0.08)_1px,transparent_1px)] [background-size:34px_34px]" />
              <div class="relative flex items-center justify-between text-[10px] font-bold uppercase tracking-[0.2em] text-[#d4a27f]">
                <span>{{ t('home.heroVisualEyebrow') }}</span>
                <span class="flex items-center gap-2 text-[#b6d59c]"><span class="h-2 w-2 animate-pulse rounded-full bg-[#8fbd70]" />{{ t('home.heroVisualLive') }}</span>
              </div>
              <div class="relative py-3 sm:py-4 lg:py-5">
                <ApiNetworkVisual
                  :upstream-nodes="heroUpstreamNodes"
                  :downstream-nodes="heroDownstreamNodes"
                  :core-subtitle="t('home.heroVisualCore')"
                  :accessible-title="t('home.heroFlowTitle')"
                  :accessible-description="t('home.heroFlowDescription')"
                />
              </div>
              <div class="relative flex items-end justify-between gap-4 border-t border-white/10 pt-5">
                <span class="max-w-[220px] text-sm leading-6 text-[#c9c3b4]">{{ t('home.heroVisualCaption') }}</span>
                <span class="font-mono text-[10px] text-[#d4a27f]">/v1 · PAYG</span>
              </div>
            </div>
          </div>
        </div>

        <div class="grid border-x border-b border-border/80 bg-background/70 sm:grid-cols-2 lg:grid-cols-4">
          <article
            v-for="(feature, index) in heroFeatures"
            :key="feature.title"
            class="feature-tile border-b border-border/70 p-6 last:border-b-0 sm:[&:nth-child(odd)]:border-r lg:border-b-0 lg:border-r lg:last:border-r-0"
          >
            <div class="text-[10px] font-bold tracking-[0.18em] text-primary">
              0{{ index + 1 }}
            </div>
            <h2 class="mt-3 text-base font-semibold">
              {{ feature.title }}
            </h2>
            <p class="mt-1 text-sm leading-6 text-muted-foreground">
              {{ feature.description }}
            </p>
          </article>
        </div>
      </section>

      <section
        :ref="el => setSectionRef(el, 1)"
        class="home-scene home-scene-tools border-y border-border/70 bg-background/55 py-12 sm:py-16 lg:flex lg:min-h-[calc(100dvh-4rem)] lg:items-center lg:snap-start"
        :class="{ 'scene-active': activeSceneIndex === 1 }"
      >
        <div class="mx-auto max-w-[1480px] px-4 sm:px-6 lg:px-8">
          <div
            class="scene-content flex flex-col justify-between gap-3 sm:flex-row sm:items-end"
            data-motion
            style="--motion-order: 0"
          >
            <div>
              <div class="section-eyebrow">
                {{ t('home.toolsEyebrow') }}
              </div>
              <h2 class="mt-3 font-serif text-3xl font-semibold sm:text-4xl">
                {{ t('home.toolsTitle') }}
              </h2>
            </div>
            <p class="max-w-md text-sm leading-6 text-muted-foreground">
              {{ t('home.toolsDescription') }}
            </p>
          </div>

          <div class="mt-8 grid gap-3 lg:grid-cols-2">
            <a
              :href="infiniteCanvasUrl"
              class="tool-tile group relative overflow-hidden border border-border/80 bg-background/75 p-7 transition hover:-translate-y-1 hover:border-primary/50 hover:shadow-md sm:p-9"
              data-motion
              style="--motion-order: 1"
            >
              <div class="absolute -right-10 -top-10 h-36 w-36 rounded-full border border-primary/20 transition duration-500 group-hover:scale-125" />
              <div class="relative flex items-start justify-between gap-6">
                <div>
                  <div class="flex items-center gap-3 text-primary"><Layers class="h-5 w-5" /><span class="text-[10px] font-bold uppercase tracking-[0.2em]">{{ t('home.canvasEyebrow') }}</span></div>
                  <h3 class="mt-7 font-serif text-3xl font-semibold">{{ t('home.canvasTitle') }}</h3>
                  <p class="mt-3 max-w-sm text-sm leading-7 text-muted-foreground">{{ t('home.canvasDescription') }}</p>
                </div>
                <ArrowUpRight class="h-5 w-5 shrink-0 text-muted-foreground transition group-hover:text-primary" />
              </div>
            </a>
            <RouterLink
              :to="imageStudioPath"
              class="tool-tile group relative overflow-hidden border border-border/80 bg-[#26231f] p-7 text-[#f7f3ea] transition hover:-translate-y-1 hover:shadow-md sm:p-9"
              data-motion
              style="--motion-order: 2"
              @click="openImageStudio"
            >
              <div class="absolute -bottom-16 -right-8 h-36 w-36 rounded-full border border-[#d4a27f]/25 transition duration-500 group-hover:scale-125">
                <ArrowDownRight
                  class="absolute left-8 top-8 h-5 w-5 text-[#d4a27f] transition group-hover:text-[#f0bb95]"
                  aria-hidden="true"
                />
              </div>
              <div class="relative flex items-start justify-between gap-6">
                <div>
                  <div class="flex items-center gap-3 text-[#d4a27f]">
                    <Sparkles class="h-5 w-5" /><span class="text-[10px] font-bold uppercase tracking-[0.2em]">{{ t('home.imageStudioEyebrow') }}</span>
                  </div>
                  <h3 class="mt-7 font-serif text-3xl font-semibold">
                    {{ t('home.imageStudioTitle') }}
                  </h3>
                  <p class="mt-3 max-w-sm text-sm leading-7 text-[#c9c3b4]">
                    {{ t('home.imageStudioDescription') }}
                  </p>
                </div>
              </div>
            </RouterLink>
          </div>

          <div
            class="tool-flow mt-3 grid overflow-hidden border border-border/80 bg-background/70 sm:grid-cols-3"
            data-motion
            style="--motion-order: 3"
          >
            <article
              v-for="(step, index) in tipsSteps"
              :key="step.title"
              class="tool-flow-step group relative flex gap-4 border-b border-border/70 p-5 last:border-b-0 sm:border-b-0 sm:border-r sm:last:border-r-0"
            >
              <span class="tool-flow-index flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-primary/25 bg-primary/5 font-mono text-xs font-bold text-primary transition group-hover:border-primary/60 group-hover:bg-primary group-hover:text-primary-foreground">
                0{{ index + 1 }}
              </span>
              <div>
                <h3 class="text-sm font-semibold">
                  {{ step.title }}
                </h3>
                <p class="mt-1 text-xs leading-5 text-muted-foreground">
                  {{ step.description }}
                </p>
              </div>
            </article>
          </div>
        </div>
      </section>

      <section
        :ref="el => setSectionRef(el, 2)"
        class="home-scene home-scene-models mx-auto max-w-[1480px] px-4 py-12 sm:px-6 lg:flex lg:min-h-[calc(100dvh-4rem)] lg:items-center lg:px-8 lg:py-16 lg:snap-start"
        :class="{ 'scene-active': activeSceneIndex === 2 }"
      >
        <div class="w-full">
          <div
            class="scene-content flex flex-col justify-between gap-4 border-b border-border/70 pb-5 sm:flex-row sm:items-end"
            data-motion
            style="--motion-order: 0"
          >
            <div>
              <div class="section-eyebrow">
                {{ t('home.modelsEyebrow') }}
              </div>
              <h2 class="mt-3 font-serif text-3xl font-semibold sm:text-4xl">
                {{ t('home.modelsTitle') }}
              </h2>
            </div>
            <RouterLink
              to="/models"
              class="inline-flex items-center gap-2 text-sm font-semibold text-primary hover:underline"
            >
              {{ t('home.viewAllModels') }} <ArrowRight class="h-4 w-4" />
            </RouterLink>
          </div>

          <div class="mt-6 grid gap-3 lg:grid-cols-[minmax(0,0.92fr)_minmax(420px,1.08fr)]">
            <div
              class="model-catalog flex min-h-[330px] flex-col border border-border/80 bg-background/75 p-6 sm:p-7"
              data-motion
              style="--motion-order: 1"
            >
              <div class="flex items-center justify-between gap-4">
                <div class="flex items-center gap-2 text-[10px] font-bold uppercase tracking-[0.18em] text-primary">
                  <span class="h-2 w-2 animate-pulse rounded-full bg-emerald-500" />
                  {{ t('home.heroVisualLive') }}
                </div>
                <span class="font-mono text-[10px] text-muted-foreground">/v1/models</span>
              </div>

              <div
                v-if="featuredModels.length"
                class="mt-6 grid gap-2 sm:grid-cols-2"
              >
                <RouterLink
                  v-for="model in featuredModels"
                  :key="model.id"
                  to="/models"
                  class="model-chip group flex min-w-0 items-center gap-3 border border-border/70 bg-background/70 px-3 py-2.5 text-sm transition hover:border-primary/50 hover:text-primary"
                >
                  <span
                    class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border bg-background text-[10px] font-semibold"
                    :class="modelBadgeClass(model.name)"
                  >
                    <img
                      v-if="modelIcon(model.name)"
                      :src="modelIcon(model.name) || undefined"
                      :alt="`${modelFamily(model.name)} icon`"
                      class="h-4 w-4 object-contain"
                    >
                    <span v-else>{{ modelInitial(model.name) }}</span>
                  </span>
                  <span class="min-w-0 flex-1 truncate">{{ model.display_name || model.name }}</span>
                  <ArrowUpRight class="h-3.5 w-3.5 shrink-0 text-muted-foreground transition group-hover:text-primary" />
                </RouterLink>
              </div>
              <div
                v-else
                class="mt-6 grid gap-2 sm:grid-cols-2"
              >
                <RouterLink
                  v-for="family in modelFamilies"
                  :key="family.name"
                  to="/models"
                  class="model-chip group flex items-center gap-3 border border-border/70 bg-background/70 px-3 py-2.5 text-sm transition hover:border-primary/50 hover:text-primary"
                >
                  <span class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-border/70 bg-background">
                    <img
                      :src="family.icon"
                      :alt="`${family.name} icon`"
                      class="h-4 w-4 object-contain"
                    >
                  </span>
                  <span class="flex-1 font-medium">{{ family.name }}</span>
                  <span
                    v-if="modelsLoading"
                    class="h-1.5 w-1.5 animate-pulse rounded-full bg-primary/60"
                  />
                  <ArrowUpRight
                    v-else
                    class="h-3.5 w-3.5 text-muted-foreground transition group-hover:text-primary"
                  />
                </RouterLink>
              </div>

              <div class="mt-auto grid grid-cols-3 divide-x divide-border/70 border-border/70 pt-5 text-center">
                <div>
                  <div class="font-serif text-2xl font-semibold">
                    {{ modelTotalLabel }}
                  </div>
                  <div class="mt-1 text-[9px] uppercase tracking-[0.15em] text-muted-foreground">
                    {{ t('home.modelsStat') }}
                  </div>
                </div>
                <div>
                  <div class="font-serif text-2xl font-semibold">
                    3
                  </div>
                  <div class="mt-1 text-[9px] uppercase tracking-[0.15em] text-muted-foreground">
                    {{ t('home.protocolsStat') }}
                  </div>
                </div>
                <div>
                  <div class="font-serif text-2xl font-semibold">
                    1
                  </div>
                  <div class="mt-1 text-[9px] uppercase tracking-[0.15em] text-muted-foreground">
                    {{ t('home.gatewayStat') }}
                  </div>
                </div>
              </div>
            </div>

            <div
              class="model-network group relative min-h-[330px] overflow-hidden bg-[#26231f] p-6 text-[#f7f3ea] sm:p-8"
              data-motion
              style="--motion-order: 2"
            >
              <div class="model-network-grid absolute inset-0 opacity-35" />
              <div class="model-network-glow pointer-events-none absolute left-1/2 top-1/2 h-56 w-56 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#d4a27f]/10 blur-3xl" />
              <div class="relative flex items-center justify-between text-[10px] font-bold uppercase tracking-[0.2em] text-[#d4a27f]">
                <span>{{ t('home.heroVisualEyebrow') }}</span>
                <span class="font-mono text-[#b6d59c]">SYNC · 100%</span>
              </div>

              <div class="model-orbit relative mx-auto mt-4 h-[230px] max-w-[500px]">
                <div class="model-orbit-ring model-orbit-ring-outer" />
                <div class="model-orbit-ring model-orbit-ring-inner" />
                <span class="model-signal model-signal-a" />
                <span class="model-signal model-signal-b" />
                <div class="model-core absolute left-1/2 top-1/2 z-10 flex h-24 w-24 -translate-x-1/2 -translate-y-1/2 flex-col items-center justify-center rounded-full border border-[#d4a27f]/50 bg-[#26231f]/90 shadow-[0_0_45px_rgba(212,162,127,0.2)] backdrop-blur">
                  <span class="font-serif text-xl font-semibold">Niffler</span>
                  <span class="mt-1 text-[8px] uppercase tracking-[0.16em] text-[#d4a27f]">{{ t('home.heroVisualCore') }}</span>
                </div>
                <div
                  v-for="(family, index) in modelFamilies"
                  :key="family.name"
                  class="model-node absolute z-20 flex items-center gap-2 rounded-full border border-white/15 bg-[#312e29]/95 px-3 py-2 text-xs shadow-lg backdrop-blur"
                  :class="`model-node-${index + 1}`"
                >
                  <img
                    :src="family.icon"
                    alt=""
                    class="h-4 w-4 object-contain"
                  >
                  <span>{{ family.name }}</span>
                </div>
              </div>
            </div>
          </div>

          <div
            class="mt-3 grid border border-border/80 bg-background/70 sm:grid-cols-2 lg:grid-cols-4"
            data-motion
            style="--motion-order: 3"
          >
            <article
              v-for="(benefit, index) in modelBenefits"
              :key="benefit.title"
              class="model-benefit border-b border-border/70 p-6 transition-colors hover:bg-primary/[0.045] sm:[&:nth-child(odd)]:border-r sm:[&:nth-child(n+3)]:border-b-0 lg:border-b-0 lg:border-r lg:last:border-r-0"
            >
              <div class="text-[10px] font-bold tracking-[0.18em] text-primary">
                0{{ index + 1 }}
              </div>
              <h3 class="mt-3 text-base font-semibold leading-6">
                {{ benefit.title }}
              </h3>
              <p class="mt-2 text-sm leading-6 text-muted-foreground">
                {{ benefit.description }}
              </p>
            </article>
          </div>
        </div>
      </section>

      <section
        :ref="el => setSectionRef(el, 3)"
        class="home-scene home-scene-faq border-y border-border/70 bg-background/55 py-12 sm:py-16 lg:flex lg:min-h-[calc(100dvh-4rem)] lg:items-center lg:snap-start"
        :class="{ 'scene-active': activeSceneIndex === 3 }"
      >
        <div class="mx-auto max-w-[1480px] px-4 sm:px-6 lg:px-8">
          <div
            class="scene-content flex flex-col justify-between gap-3 sm:flex-row sm:items-end"
            data-motion
            style="--motion-order: 0"
          >
            <div>
              <div class="section-eyebrow">
                {{ t('home.faqEyebrow') }}
              </div>
              <h2 class="mt-3 font-serif text-3xl font-semibold sm:text-4xl">
                {{ t('home.faqTitle') }}
              </h2>
            </div>
            <RouterLink
              to="/guide/faq"
              class="inline-flex items-center gap-2 text-sm font-semibold text-primary hover:underline"
            >
              {{ t('home.faqMore') }} <ArrowUpRight class="h-4 w-4" />
            </RouterLink>
          </div>

          <div
            class="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1.35fr)_minmax(320px,0.65fr)] lg:items-stretch"
            data-motion
            style="--motion-order: 1"
          >
            <div class="divide-y divide-border/70 border-y border-border/70">
              <div
                v-for="faq in faqItems"
                :key="faq.id"
              >
                <button
                  class="flex w-full items-center justify-between gap-6 py-5 text-left text-sm font-semibold transition hover:text-primary sm:text-base"
                  :aria-expanded="openFaqId === faq.id"
                  @click="openFaqId = openFaqId === faq.id ? null : faq.id"
                >
                  <span>{{ faq.question }}</span>
                  <ChevronDown
                    class="h-4 w-4 shrink-0 text-muted-foreground transition-transform"
                    :class="openFaqId === faq.id ? 'rotate-180 text-primary' : ''"
                  />
                </button>
                <div
                  v-if="openFaqId === faq.id"
                  class="pb-5 pr-10 text-sm leading-7 text-muted-foreground"
                >
                  {{ faq.answer }}
                </div>
              </div>
            </div>

            <aside class="quick-panel group relative flex min-h-[350px] flex-col overflow-hidden bg-[#26231f] p-7 text-[#f7f3ea] sm:p-8">
              <div class="motion-orbit pointer-events-none absolute -right-16 -top-16 h-48 w-48 rounded-full border border-[#d4a27f]/20 transition duration-700 group-hover:scale-110" />
              <div class="pointer-events-none absolute -bottom-24 -left-20 h-52 w-52 rounded-full bg-[#d4a27f]/10 blur-3xl" />
              <div class="relative">
                <div class="text-[10px] font-bold uppercase tracking-[0.2em] text-[#d4a27f]">
                  {{ t('home.ctaEyebrow') }}
                </div>
                <h3 class="mt-3 font-serif text-3xl font-semibold">
                  {{ t('home.ctaTitle') }}
                </h3>
                <p class="mt-3 max-w-sm text-sm leading-7 text-[#c9c3b4]">
                  {{ t('home.ctaDescription') }}
                </p>
              </div>

              <div class="relative mt-7 flex flex-col gap-3 sm:flex-row">
                <button
                  class="inline-flex h-11 items-center justify-center gap-2 bg-[#d4a27f] px-5 text-sm font-semibold text-[#26231f] transition hover:-translate-y-0.5 hover:bg-[#e1b795]"
                  @click="openPrimaryAction"
                >
                  {{ authStore.isAuthenticated ? t('nav.dashboard') : t('home.startNow') }}
                  <ArrowRight class="h-4 w-4" />
                </button>
                <RouterLink
                  to="/models"
                  class="inline-flex h-11 items-center justify-center gap-2 border border-white/15 px-5 text-sm font-semibold transition hover:border-[#d4a27f]/60 hover:text-[#d4a27f]"
                >
                  {{ t('home.exploreModels') }}
                  <ArrowUpRight class="h-4 w-4" />
                </RouterLink>
              </div>

              <div class="relative mt-auto flex flex-wrap gap-x-4 gap-y-2 border-t border-white/10 pt-6 text-xs text-[#c9c3b4]">
                <span
                  v-for="proof in heroProofs"
                  :key="proof"
                  class="flex items-center gap-2"
                >
                  <CheckCircle2 class="h-4 w-4 text-[#b6d59c]" />{{ proof }}
                </span>
              </div>
            </aside>
          </div>
        </div>
      </section>
    </main>

    <Transition name="tips">
      <aside
        v-if="showTipsCard"
        class="fixed bottom-5 right-5 z-40 w-[min(calc(100vw-2rem),28rem)] border border-primary/30 bg-popover p-5 text-popover-foreground shadow-[0_16px_48px_hsl(var(--foreground)/0.18)] sm:bottom-6 sm:right-6 sm:p-6"
        role="dialog"
        :aria-labelledby="tipsTitleId"
      >
        <div class="flex items-start justify-between gap-4">
          <div>
            <div class="section-eyebrow">
              {{ t('home.tipsEyebrow') }}
            </div>
            <h2
              :id="tipsTitleId"
              class="mt-2 font-serif text-2xl font-semibold"
            >
              {{ t('home.tipsTitle') }}
            </h2>
          </div>
          <button
            class="inline-flex h-8 w-8 shrink-0 items-center justify-center text-muted-foreground transition hover:bg-muted hover:text-foreground"
            :aria-label="t('home.tipsClose')"
            @click="closeTips"
          >
            <X class="h-4 w-4" />
          </button>
        </div>
        <ol class="mt-5 space-y-3">
          <li
            v-for="(step, index) in tipsSteps"
            :key="step.title"
            class="flex gap-3"
          >
            <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary/10 text-xs font-bold text-primary">{{ index + 1 }}</span>
            <div>
              <div class="text-sm font-semibold">
                {{ step.title }}
              </div>
              <p class="mt-0.5 text-xs leading-5 text-muted-foreground">
                {{ step.description }}
              </p>
            </div>
          </li>
        </ol>
        <button
          class="mt-5 inline-flex h-10 w-full items-center justify-center gap-2 bg-primary px-4 text-sm font-semibold text-primary-foreground transition hover:bg-primary/90"
          @click="openPrimaryAction"
        >
          {{ t('home.tipsAction') }} <ArrowRight class="h-4 w-4" />
        </button>
      </aside>
    </Transition>

    <button
      v-if="!showTipsCard"
      ref="tipsLauncher"
      class="fixed bottom-5 right-5 z-40 inline-flex h-11 touch-none select-none items-center gap-2 border border-primary bg-primary px-4 text-sm font-semibold text-primary-foreground shadow-[0_8px_24px_hsl(var(--primary)/0.28)] transition-[background-color,box-shadow] hover:bg-primary/90 sm:bottom-6 sm:right-6"
      :class="launcherDragging ? 'cursor-grabbing shadow-[0_14px_34px_hsl(var(--primary)/0.38)]' : 'cursor-grab'"
      :style="tipsLauncherStyle"
      @click="handleTipsLauncherClick"
      @pointerdown="startTipsLauncherDrag"
      @pointermove="moveTipsLauncher"
      @pointerup="endTipsLauncherDrag"
      @pointercancel="cancelTipsLauncherDrag"
    >
      <HelpCircle class="h-4 w-4" />{{ t('home.tipsReopen') }}
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, type ComponentPublicInstance } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { ArrowDownRight, ArrowRight, ArrowUpRight, CheckCircle2, ChevronDown, HelpCircle, Layers, Sparkles, X } from 'lucide-vue-next'
import { useAuthStore } from '@/stores/auth'
import { usePublicLoginDialog } from '@/composables/usePublicLoginDialog'
import { useSiteInfo } from '@/composables/useSiteInfo'
import { getPublicGlobalModels, type PublicGlobalModel } from '@/api/public-models'
import { getInfiniteCanvasUrl } from '@/utils/infiniteCanvasUrl'
import ApiNetworkVisual from '@/components/home/ApiNetworkVisual.vue'
import HomeCinematicVisual, {
  type HomeCinematicScene,
  type HomeScrollDirection,
} from '@/components/home/HomeCinematicVisual.vue'

const PublicEndpointLatency = defineAsyncComponent(
  () => import('@/components/home/PublicEndpointLatency.vue'),
)

const router = useRouter()
const { t } = useI18n()
const authStore = useAuthStore()
const { showLoginDialog } = usePublicLoginDialog()
const { portal } = useSiteInfo()
const models = ref<PublicGlobalModel[]>([])
const modelsLoading = ref(true)
const openFaqId = ref<number | null>(null)
const showTipsCard = ref(false)
const homeRoot = ref<HTMLElement | null>(null)
const tipsLauncher = ref<HTMLButtonElement | null>(null)
const tipsLauncherPosition = ref<{ x: number, y: number } | null>(null)
const launcherDragging = ref(false)
const tipsTitleId = 'home-tips-title'
const sceneIds: HomeCinematicScene[] = ['hero', 'tools', 'models', 'faq']
const sectionRefs = ref<Array<HTMLElement | null>>(sceneIds.map(() => null))
const activeSceneIndex = ref(0)
const previousSceneIndex = ref(0)
const scrollDirection = ref<HomeScrollDirection>('down')
const reducedMotion = ref(false)
const desktopMotionEnabled = ref(false)
let lastScrollTop = 0
let scrollAnimationFrame: number | null = null
let reducedMotionQuery: MediaQueryList | null = null
let desktopMotionQuery: MediaQueryList | null = null
let launcherPointerId: number | null = null
let launcherDragStart = { x: 0, y: 0 }
let launcherDragOrigin = { x: 0, y: 0 }
let launcherWasDragged = false

const dashboardPath = computed(() => authStore.canAccessAdmin ? '/admin/dashboard' : '/dashboard')
const defaultPortalBaseDomain = computed(() => {
  if (portal.value?.id !== 'default' || !portal.value.canonical_url) return ''
  try {
    return new URL(portal.value.canonical_url).hostname
  } catch {
    return ''
  }
})
const showPublicEndpointLatency = computed(() => defaultPortalBaseDomain.value.length > 0)
const imageStudioPath = computed(() => authStore.canAccessAdmin ? '/admin/image-studio' : '/dashboard/image-studio')
const infiniteCanvasUrl = getInfiniteCanvasUrl('canvas')
const activeScene = computed<HomeCinematicScene>(() => sceneIds[activeSceneIndex.value] || 'hero')
const tipsLauncherStyle = computed(() => tipsLauncherPosition.value
  ? {
      left: `${tipsLauncherPosition.value.x}px`,
      top: `${tipsLauncherPosition.value.y}px`,
      right: 'auto',
      bottom: 'auto',
    }
  : undefined)
const sceneLabels = computed(() => [
  t('home.heroEyebrow'),
  t('home.toolsTitle'),
  t('home.modelsTitle'),
  t('home.faqTitle'),
])

const heroUpstreamNodes = computed(() => [
  { id: 'gpt', label: 'GPT', icon: '/openai.svg', tone: '#10A37F' },
  { id: 'claude', label: 'Claude', icon: '/claude-color.svg', tone: '#D97757' },
  { id: 'gemini', label: 'Gemini', icon: '/gemini-color.svg', tone: '#4285F4' },
  { id: 'deepseek', label: 'DeepSeek', icon: '/deepseek.svg', tone: '#4B8BEA' },
  { id: 'qwen', label: 'Qwen', icon: '/qwen.svg', tone: '#7667E8' },
  { id: 'more', label: t('home.moreModels'), glyph: '···', tone: '#D4A27F' },
])

const heroDownstreamNodes = computed(() => [
  { id: 'codex', label: 'Codex', icon: '/openai.svg', tone: '#10A37F' },
  { id: 'claude-code', label: 'Claude Code', icon: '/claude-color.svg', tone: '#D97757' },
  { id: 'canvas', label: t('home.canvasTitle'), glyph: '∞', tone: '#B6D59C' },
  { id: 'image-studio', label: t('home.imageStudioTitle'), glyph: '✦', tone: '#D4A27F' },
])

const modelFamilies = [
  { name: 'GPT', icon: '/openai.svg' },
  { name: 'Claude', icon: '/claude-color.svg' },
  { name: 'Gemini', icon: '/gemini-color.svg' },
  { name: 'DeepSeek', icon: '/deepseek.svg' },
  { name: 'Qwen', icon: '/qwen.svg' },
]

const heroProofs = computed(() => [
  t('home.openaiCompatible'),
  t('home.payAsYouGo'),
  t('home.noSdkChanges'),
])

const heroFeatures = computed(() => [
  { title: t('home.featurePriceTitle'), description: t('home.featurePriceDescription') },
  { title: t('home.featurePaygTitle'), description: t('home.featurePaygDescription') },
  { title: t('home.featureOneKeyTitle'), description: t('home.featureOneKeyDescription') },
  { title: t('home.featureToolsTitle'), description: t('home.featureToolsDescription') },
])

const modelBenefits = computed(() => [
  { title: t('home.modelsBenefitOneKeyTitle'), description: t('home.modelsBenefitOneKeyDescription') },
  { title: t('home.modelsBenefitRoutingTitle'), description: t('home.modelsBenefitRoutingDescription') },
  { title: t('home.modelsBenefitQuotaTitle'), description: t('home.modelsBenefitQuotaDescription') },
  { title: t('home.modelsBenefitTraceTitle'), description: t('home.modelsBenefitTraceDescription') },
])

const faqItems = computed(() => [1, 2, 3, 4, 5].map(id => ({
  id,
  question: t(`home.faqQ${id}`),
  answer: t(`home.faqA${id}`),
})))

const tipsSteps = computed(() => [1, 2, 3].map(id => ({
  title: t(`home.tipsStep${id}Title`),
  description: t(`home.tipsStep${id}Description`),
})))

const featuredModels = computed(() => {
  const groups = new Map<string, PublicGlobalModel[]>()
  for (const model of models.value) {
    const family = modelFamily(model.name)
    const group = groups.get(family) || []
    group.push(model)
    groups.set(family, group)
  }
  const selected: PublicGlobalModel[] = []
  let index = 0
  while (selected.length < 8 && selected.length < models.value.length) {
    let added = false
    for (const group of groups.values()) {
      if (group[index]) {
        selected.push(group[index])
        added = true
        if (selected.length === 8) break
      }
    }
    if (!added) break
    index += 1
  }
  return selected
})

const modelTotalLabel = computed(() => models.value.length ? String(models.value.length) : `${modelFamilies.length}+`)

function openPrimaryAction() {
  if (authStore.isAuthenticated) void router.push(dashboardPath.value)
  else showLoginDialog.value = true
}

function openImageStudio(event: MouseEvent) {
  if (authStore.isAuthenticated) return

  event.preventDefault()
  showLoginDialog.value = true
}

function closeTips() {
  showTipsCard.value = false
}

function openTips() {
  showTipsCard.value = true
}

function clampTipsLauncherPosition(x: number, y: number) {
  const launcher = tipsLauncher.value
  if (!launcher || typeof window === 'undefined') return { x, y }

  const edge = 8
  const headerOffset = window.innerWidth >= 1024 ? 72 : 8
  return {
    x: Math.min(Math.max(x, edge), Math.max(edge, window.innerWidth - launcher.offsetWidth - edge)),
    y: Math.min(Math.max(y, headerOffset), Math.max(headerOffset, window.innerHeight - launcher.offsetHeight - edge)),
  }
}

function startTipsLauncherDrag(event: PointerEvent) {
  if (event.button !== 0 || !tipsLauncher.value) return

  const rect = tipsLauncher.value.getBoundingClientRect()
  launcherPointerId = event.pointerId
  launcherDragStart = { x: event.clientX, y: event.clientY }
  launcherDragOrigin = { x: rect.left, y: rect.top }
  launcherWasDragged = false
  launcherDragging.value = true
  tipsLauncherPosition.value = { x: rect.left, y: rect.top }
  tipsLauncher.value.setPointerCapture?.(event.pointerId)
}

function moveTipsLauncher(event: PointerEvent) {
  if (!launcherDragging.value || event.pointerId !== launcherPointerId) return

  const deltaX = event.clientX - launcherDragStart.x
  const deltaY = event.clientY - launcherDragStart.y
  if (Math.hypot(deltaX, deltaY) > 4) launcherWasDragged = true
  tipsLauncherPosition.value = clampTipsLauncherPosition(
    launcherDragOrigin.x + deltaX,
    launcherDragOrigin.y + deltaY,
  )
  if (launcherWasDragged) event.preventDefault()
}

function finishTipsLauncherDrag(event: PointerEvent) {
  if (event.pointerId !== launcherPointerId) return
  tipsLauncher.value?.releasePointerCapture?.(event.pointerId)
  launcherDragging.value = false
  launcherPointerId = null
}

function endTipsLauncherDrag(event: PointerEvent) {
  finishTipsLauncherDrag(event)
}

function cancelTipsLauncherDrag(event: PointerEvent) {
  launcherWasDragged = true
  finishTipsLauncherDrag(event)
}

function handleTipsLauncherClick() {
  if (launcherWasDragged) {
    launcherWasDragged = false
    return
  }
  openTips()
}

function keepTipsLauncherInViewport() {
  if (!tipsLauncherPosition.value) return
  tipsLauncherPosition.value = clampTipsLauncherPosition(
    tipsLauncherPosition.value.x,
    tipsLauncherPosition.value.y,
  )
}

function modelInitial(name: string) {
  const family = modelFamily(name)
  if (family === 'claude' || family === 'codex') return 'C'
  if (family === 'gpt' || family === 'image') return 'G'
  if (family === 'gemini') return '✦'
  if (family === 'deepseek') return 'D'
  if (family === 'qwen') return 'Q'
  return name.slice(0, 1).toUpperCase()
}

function modelBadgeClass(name: string) {
  const family = modelFamily(name)
  if (family === 'claude' || family === 'codex') return 'border-[#d97757]/35 bg-[#d97757]/10 text-[#c65f3d]'
  if (family === 'gpt' || family === 'image') return 'border-[#10a37f]/35 bg-[#10a37f]/10 text-[#087f63]'
  if (family === 'gemini') return 'border-[#4285f4]/35 bg-[#4285f4]/10 text-[#3574d3]'
  if (family === 'deepseek') return 'border-[#4b8bea]/35 bg-[#4b8bea]/10 text-[#3675c9]'
  if (family === 'qwen') return 'border-[#6155d9]/35 bg-[#6155d9]/10 text-[#5145bf]'
  return 'border-primary/25 bg-primary/10 text-primary'
}

function modelIcon(name: string): string | null {
  const family = modelFamily(name)
  if (family === 'claude') return '/claude-color.svg'
  if (family === 'gemini') return '/gemini-color.svg'
  if (family === 'gpt' || family === 'image' || family === 'codex') return '/openai.svg'
  if (family === 'deepseek') return '/deepseek.svg'
  if (family === 'doubao') return '/doubao.svg'
  if (family === 'glm') return '/glm.svg'
  if (family === 'grok') return '/grok.svg'
  if (family === 'kimi') return '/kimi.svg'
  if (family === 'mimo') return '/mimo.svg'
  if (family === 'minimax') return '/minimax.svg'
  if (family === 'qwen') return '/qwen.svg'
  if (family === 'wenxin') return '/wenxin.svg'
  return null
}

function modelFamily(name: string) {
  const normalized = name.toLowerCase()
  if (normalized.startsWith('claude')) return 'claude'
  if (normalized.startsWith('codex')) return 'codex'
  if (normalized.startsWith('gpt-image')) return 'image'
  if (normalized.startsWith('gpt') || normalized.startsWith('o1') || normalized.startsWith('o3')) return 'gpt'
  if (normalized.startsWith('gemini')) return 'gemini'
  if (normalized.startsWith('deepseek')) return 'deepseek'
  if (normalized.startsWith('doubao')) return 'doubao'
  if (normalized.startsWith('glm') || normalized.startsWith('chatglm') || normalized.startsWith('zhipu')) return 'glm'
  if (normalized.startsWith('grok')) return 'grok'
  if (normalized.startsWith('kimi') || normalized.startsWith('moonshot')) return 'kimi'
  if (normalized.startsWith('mimo') || normalized.startsWith('xiaomi')) return 'mimo'
  if (normalized.startsWith('minimax')) return 'minimax'
  if (normalized.startsWith('qwen')) return 'qwen'
  if (normalized.startsWith('wenxin') || normalized.startsWith('ernie') || normalized.startsWith('baidu')) return 'wenxin'
  return normalized.split(/[-/:]/)[0] || normalized
}

async function loadModels() {
  modelsLoading.value = true
  try {
    const firstPage = await getPublicGlobalModels({ skip: 0, limit: 1000, is_active: true })
    const collected = [...(firstPage.models || [])]
    while (collected.length < firstPage.total) {
      const page = await getPublicGlobalModels({ skip: collected.length, limit: 1000, is_active: true })
      if (!page.models?.length) break
      const knownIds = new Set(collected.map(model => model.id))
      const additions = page.models.filter(model => !knownIds.has(model.id))
      if (!additions.length) break
      collected.push(...additions)
    }
    models.value = collected
  } catch {
    models.value = []
  } finally {
    modelsLoading.value = false
  }
}

function setSectionRef(element: Element | ComponentPublicInstance | null, index: number) {
  sectionRefs.value[index] = element instanceof HTMLElement ? element : null
}

function updateActiveScene() {
  const container = homeRoot.value
  if (!container || !desktopMotionEnabled.value) return

  const nextScrollTop = container.scrollTop
  if (nextScrollTop !== lastScrollTop) {
    scrollDirection.value = nextScrollTop > lastScrollTop ? 'down' : 'up'
  }
  lastScrollTop = nextScrollTop

  const sectionOffsets = sectionRefs.value.map(section => section?.offsetTop ?? 0)
  if (sectionOffsets.every(offset => offset === 0)) return

  const scrollMiddle = nextScrollTop + container.clientHeight / 2
  let nextSceneIndex = 0
  for (let index = sectionRefs.value.length - 1; index >= 0; index -= 1) {
    const section = sectionRefs.value[index]
    if (section && section.offsetTop <= scrollMiddle) {
      nextSceneIndex = index
      break
    }
  }

  if (nextSceneIndex === activeSceneIndex.value) return
  previousSceneIndex.value = activeSceneIndex.value
  activeSceneIndex.value = nextSceneIndex
}

function scheduleSceneUpdate() {
  if (scrollAnimationFrame !== null) return

  if (typeof window.requestAnimationFrame !== 'function') {
    updateActiveScene()
    return
  }

  scrollAnimationFrame = window.requestAnimationFrame(() => {
    scrollAnimationFrame = null
    updateActiveScene()
  })
}

function scrollToScene(index: number) {
  const target = sectionRefs.value[index]
  if (!target) return

  const behavior = reducedMotion.value ? 'auto' : 'smooth'
  const container = homeRoot.value
  if (desktopMotionEnabled.value && container && typeof container.scrollTo === 'function') {
    container.scrollTo({ top: target.offsetTop, behavior })
    return
  }

  target.scrollIntoView?.({ behavior, block: 'start' })
}

function applyDesktopMotion(enabled: boolean) {
  desktopMotionEnabled.value = enabled
  const container = homeRoot.value
  if (!container) return

  container.classList.toggle('motion-ready', enabled && !reducedMotion.value)
  if (!enabled) {
    activeSceneIndex.value = 0
    previousSceneIndex.value = 0
    lastScrollTop = 0
    return
  }

  updateActiveScene()
}

function handleReducedMotionChange(event: MediaQueryListEvent) {
  reducedMotion.value = event.matches
  applyDesktopMotion(desktopMotionEnabled.value)
}

function handleDesktopMotionChange(event: MediaQueryListEvent) {
  applyDesktopMotion(event.matches)
}

async function setupCinematicMotion() {
  if (!homeRoot.value || typeof window === 'undefined') return

  await nextTick()
  reducedMotionQuery = window.matchMedia?.('(prefers-reduced-motion: reduce)') || null
  desktopMotionQuery = window.matchMedia?.('(min-width: 1024px)') || null
  reducedMotion.value = reducedMotionQuery?.matches ?? false
  applyDesktopMotion(desktopMotionQuery?.matches ?? window.innerWidth >= 1024)

  homeRoot.value.addEventListener('scroll', scheduleSceneUpdate, { passive: true })
  reducedMotionQuery?.addEventListener?.('change', handleReducedMotionChange)
  desktopMotionQuery?.addEventListener?.('change', handleDesktopMotionChange)
}

onMounted(() => {
  void loadModels()
  void setupCinematicMotion()
  window.addEventListener('resize', keepTipsLauncherInViewport)
})

onBeforeUnmount(() => {
  homeRoot.value?.removeEventListener('scroll', scheduleSceneUpdate)
  reducedMotionQuery?.removeEventListener?.('change', handleReducedMotionChange)
  desktopMotionQuery?.removeEventListener?.('change', handleDesktopMotionChange)
  window.removeEventListener('resize', keepTipsLauncherInViewport)
  if (scrollAnimationFrame !== null && typeof window.cancelAnimationFrame === 'function') {
    window.cancelAnimationFrame(scrollAnimationFrame)
  }
})
</script>

<style scoped>
.home-scroll-root {
  scrollbar-color: color-mix(in oklab, var(--primary) 40%, transparent) transparent;
  scrollbar-width: thin;
}

.home-scroll-root::-webkit-scrollbar { width: 7px; }
.home-scroll-root::-webkit-scrollbar-track { background: transparent; }
.home-scroll-root::-webkit-scrollbar-thumb {
  border-radius: 9999px;
  background: color-mix(in oklab, var(--primary) 32%, transparent);
}

.home-scene {
  position: relative;
  isolation: isolate;
}

.home-scene > * {
  position: relative;
  z-index: 1;
  width: 100%;
}

.scene-indicator {
  position: fixed;
  right: 0.5rem;
  top: calc(50% + 2rem);
  z-index: 30;
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  transform: translateY(-50%);
}

.scene-indicator-button {
  position: relative;
  display: flex;
  width: 1.75rem;
  height: 1.75rem;
  align-items: center;
  justify-content: center;
  border-radius: 9999px;
}

.scene-indicator-label {
  position: absolute;
  right: 2rem;
  min-width: max-content;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  background: color-mix(in oklab, var(--background) 88%, transparent);
  padding: 0.35rem 0.6rem;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
  letter-spacing: 0.04em;
  opacity: 0;
  transform: translateX(8px);
  transition: opacity 180ms ease, transform 220ms ease;
  pointer-events: none;
  backdrop-filter: blur(12px);
}

.scene-indicator-button:hover .scene-indicator-label,
.scene-indicator-button:focus-visible .scene-indicator-label {
  opacity: 1;
  transform: translateX(0);
}

.scene-indicator-button:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}

.scene-indicator-dot {
  width: 0.55rem;
  height: 0.55rem;
  border: 1.5px solid color-mix(in oklab, var(--muted-foreground) 44%, transparent);
  border-radius: 9999px;
  background: color-mix(in oklab, var(--background) 80%, transparent);
  box-shadow: 0 0 0 0 transparent;
  transition:
    width 280ms cubic-bezier(0.16, 1, 0.3, 1),
    height 280ms cubic-bezier(0.16, 1, 0.3, 1),
    border-color 220ms ease,
    background-color 220ms ease,
    box-shadow 280ms ease;
}

.scene-indicator-dot.active {
  width: 0.78rem;
  height: 0.78rem;
  border-color: var(--primary);
  background: var(--primary);
  box-shadow:
    0 0 0 5px color-mix(in oklab, var(--primary) 13%, transparent),
    0 0 22px color-mix(in oklab, var(--primary) 28%, transparent);
}

.hero-shell { animation: hero-in 520ms ease-out both; }
.hero-glow { animation: glow-drift 8s ease-in-out infinite; }
.hero-copy-panel > * { animation: hero-copy-in 620ms cubic-bezier(0.22, 1, 0.36, 1) both; }
.hero-copy-panel > :nth-child(1) { animation-delay: 80ms; }
.hero-copy-panel > :nth-child(2) { animation-delay: 150ms; }
.hero-copy-panel > :nth-child(3) { animation-delay: 220ms; }
.hero-copy-panel > :nth-child(4) { animation-delay: 290ms; }
.hero-copy-panel > :nth-child(5) { animation-delay: 360ms; }
.hero-grid { animation: grid-drift 12s linear infinite; }
.hero-visual {
  transform: perspective(1200px) rotateY(-1.8deg) scale(0.985);
  transform-origin: center right;
  box-shadow: 0 26px 90px rgb(0 0 0 / 0.16), inset 0 0 70px rgb(212 162 127 / 0.035);
  transition: transform 820ms cubic-bezier(0.16, 1, 0.3, 1), box-shadow 650ms ease;
}

.home-scene-hero.scene-active .hero-visual {
  transform: perspective(1200px) rotateY(0deg) scale(1);
  box-shadow: 0 30px 110px rgb(0 0 0 / 0.22), inset 0 0 90px rgb(212 162 127 / 0.065);
}

.home-scene-hero:not(.scene-active) .hero-visual {
  transform: perspective(1200px) rotateY(3deg) scale(0.95) translateX(18px);
}

.feature-tile { transition: transform 240ms ease, background-color 240ms ease; }
.feature-tile:hover { background: hsl(var(--primary) / 0.045); transform: translateY(-3px); }
.tool-tile::after {
  position: absolute;
  inset: -70% auto -70% -45%;
  width: 28%;
  content: '';
  pointer-events: none;
  background: linear-gradient(90deg, transparent, hsl(var(--primary) / 0.12), transparent);
  transform: skewX(-18deg) translateX(-240%);
  transition: transform 700ms ease;
}
.tool-tile:hover::after { transform: skewX(-18deg) translateX(620%); }
.tool-tile {
  transform-style: preserve-3d;
  transform-origin: center;
}

.tool-flow {
  box-shadow: 0 16px 50px color-mix(in oklab, var(--foreground) 5%, transparent);
}

.tool-flow-step::after {
  position: absolute;
  right: -0.32rem;
  top: 50%;
  z-index: 2;
  display: none;
  width: 0.62rem;
  height: 0.62rem;
  border-top: 1px solid color-mix(in oklab, var(--primary) 35%, var(--border));
  border-right: 1px solid color-mix(in oklab, var(--primary) 35%, var(--border));
  background: hsl(var(--background));
  content: '';
  transform: translateY(-50%) rotate(45deg);
}

.model-chip {
  position: relative;
  overflow: hidden;
}

.model-chip::after {
  position: absolute;
  inset: 0;
  background: linear-gradient(105deg, transparent 25%, hsl(var(--primary) / 0.06), transparent 75%);
  content: '';
  transform: translateX(-120%);
  transition: transform 500ms ease;
}

.model-chip:hover::after { transform: translateX(120%); }

.model-network-grid {
  background-image:
    linear-gradient(rgb(247 243 234 / 0.07) 1px, transparent 1px),
    linear-gradient(90deg, rgb(247 243 234 / 0.07) 1px, transparent 1px);
  background-size: 28px 28px;
  animation: grid-drift 12s linear infinite;
}

.model-orbit-ring {
  position: absolute;
  left: 50%;
  top: 50%;
  border: 1px solid rgb(212 162 127 / 0.24);
  border-radius: 9999px;
  transform: translate(-50%, -50%);
}

.model-orbit-ring::before,
.model-orbit-ring::after {
  position: absolute;
  width: 5px;
  height: 5px;
  border-radius: 9999px;
  background: #d4a27f;
  box-shadow: 0 0 14px rgb(212 162 127 / 0.9);
  content: '';
}

.model-orbit-ring-outer {
  width: 84%;
  height: 78%;
  animation: model-orbit-spin 16s linear infinite;
}

.model-orbit-ring-outer::before { left: 12%; top: 7%; }
.model-orbit-ring-outer::after { bottom: 6%; right: 14%; }

.model-orbit-ring-inner {
  width: 55%;
  height: 52%;
  border-color: rgb(182 213 156 / 0.18);
  animation: model-orbit-spin-reverse 12s linear infinite;
}

.model-orbit-ring-inner::before { right: 3%; top: 28%; background: #b6d59c; }
.model-orbit-ring-inner::after { bottom: 4%; left: 28%; background: #b6d59c; }

.model-signal {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 38%;
  height: 1px;
  background: linear-gradient(90deg, rgb(212 162 127 / 0.7), transparent);
  transform-origin: left center;
  opacity: 0.55;
}

.model-signal-a { transform: rotate(-28deg); }
.model-signal-b { transform: rotate(154deg); }

.model-node {
  transition: border-color 220ms ease, transform 300ms cubic-bezier(0.16, 1, 0.3, 1), box-shadow 220ms ease;
}

.model-node:hover {
  border-color: rgb(212 162 127 / 0.55);
  box-shadow: 0 8px 28px rgb(0 0 0 / 0.28), 0 0 20px rgb(212 162 127 / 0.12);
}

.model-node-1 { left: 4%; top: 15%; }
.model-node-2 { right: 2%; top: 12%; }
.model-node-3 { right: 0; top: 62%; }
.model-node-4 { bottom: 1%; left: 25%; }
.model-node-5 { left: 0; top: 58%; }
.model-node-1:hover,
.model-node-4:hover,
.model-node-5:hover { transform: translateX(3px) scale(1.04); }
.model-node-2:hover,
.model-node-3:hover { transform: translateX(-3px) scale(1.04); }

.motion-ready .home-scene.scene-active .tool-tile:hover {
  transform: perspective(1100px) translateY(-7px) rotateX(1.4deg) scale(1.012);
  box-shadow: 0 24px 72px color-mix(in oklab, var(--foreground) 13%, transparent);
}

.motion-orbit { animation: orbit-breathe 6s ease-in-out infinite; }
.quick-panel { box-shadow: 0 18px 60px rgb(38 35 31 / 0.1); }
.section-eyebrow {
  color: hsl(var(--primary));
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.2em;
  text-transform: uppercase;
}
@keyframes hero-in {
  from { opacity: 0; transform: translateY(10px); }
  to { opacity: 1; transform: translateY(0); }
}
@keyframes hero-copy-in {
  from { opacity: 0; transform: translateY(14px); }
  to { opacity: 1; transform: translateY(0); }
}
@keyframes glow-drift {
  0%, 100% { transform: translate3d(0, 0, 0); opacity: 0.8; }
  50% { transform: translate3d(-18px, 14px, 0); opacity: 1; }
}
@keyframes grid-drift {
  from { background-position: 0 0; }
  to { background-position: 34px 34px; }
}
@keyframes orbit-breathe {
  0%, 100% { opacity: 0.55; transform: scale(1); }
  50% { opacity: 1; transform: scale(1.08); }
}
@keyframes model-orbit-spin {
  from { transform: translate(-50%, -50%) rotate(0deg); }
  to { transform: translate(-50%, -50%) rotate(360deg); }
}
@keyframes model-orbit-spin-reverse {
  from { transform: translate(-50%, -50%) rotate(360deg); }
  to { transform: translate(-50%, -50%) rotate(0deg); }
}
.tips-enter-active, .tips-leave-active { transition: opacity 180ms ease, transform 180ms ease; }
.tips-enter-from, .tips-leave-to { opacity: 0; transform: translateY(8px); }

@media (min-width: 1024px) {
  .tool-flow-step::after { display: block; }
  .tool-flow-step:last-child::after { display: none; }

  .motion-ready .home-scene [data-motion] {
    will-change: transform, opacity, filter;
    transition:
      opacity 660ms cubic-bezier(0.16, 1, 0.3, 1),
      transform 760ms cubic-bezier(0.16, 1, 0.3, 1),
      filter 620ms ease;
    transition-delay: calc(var(--motion-order, 0) * 85ms);
  }

  .motion-ready .home-scene:not(.scene-active) [data-motion] {
    opacity: 0;
    filter: blur(8px);
    transition-delay: 0ms;
  }

  .motion-ready.direction-down .home-scene:not(.scene-active) [data-motion] {
    transform: translate3d(38px, 30px, 0) scale(0.965);
  }

  .motion-ready.direction-up .home-scene:not(.scene-active) [data-motion] {
    transform: translate3d(-38px, -24px, 0) scale(0.965);
  }

  .motion-ready .home-scene.scene-active [data-motion] {
    opacity: 1;
    filter: blur(0);
    transform: translate3d(0, 0, 0) scale(1);
  }
}

@media (max-width: 1023px) {
  .scene-indicator { display: none; }
  .hero-visual,
  .home-scene-hero.scene-active .hero-visual,
  .home-scene-hero:not(.scene-active) .hero-visual { transform: none; }
}

@media (prefers-reduced-motion: reduce) {
  .hero-shell, .hero-glow, .hero-copy-panel > *, .hero-grid, .motion-orbit, .model-network-grid, .model-orbit-ring { animation: none; }
  .home-scroll-root { scroll-behavior: auto; }
  .feature-tile, .tool-tile, .tool-tile::after, .hero-visual, .motion-ready [data-motion] { transition: none; }
  .motion-ready .home-scene [data-motion] { opacity: 1; filter: none; transform: none; }
  .tips-enter-active, .tips-leave-active { transition: none; }
}
</style>

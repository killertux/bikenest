/* BikeNest Alpine components — CSP build (plans/m7-hardening.md §3.3).
 *
 * The CSP build forbids `eval`/`new Function`, so every inline `x-data` object
 * literal, assignment-style `@click`, arrow-function or global access has been
 * moved out of the templates and into registered components below. The HTML
 * only references component data/methods through *simple* property-access and
 * method-call expressions, which the CSP evaluator parses without eval.
 *
 * Components must be registered before Alpine starts, so this file (loaded
 * synchronously, before the deferred Alpine script) only registers an
 * `alpine:init` listener — the callback runs once the `Alpine` global exists.
 */
document.addEventListener('alpine:init', function () {
  var Alpine = window.Alpine;

  /* ---- base.html: mobile menu -------------------------------------------------- */
  Alpine.data('mobileMenu', function () {
    return {
      open: false,
      toggle: function () { this.open = !this.open; },
    };
  });

  /* ---- home.html: hero "use my location" --------------------------------------- */
  Alpine.data('homeHero', function () {
    return {
      locating: false,
      denied: false,
      locate: function () {
        var self = this;
        if (!navigator.geolocation) { self.denied = true; return; }
        self.locating = true;
        navigator.geolocation.getCurrentPosition(
          function (pos) {
            window.location =
              '/search?lat=' + pos.coords.latitude.toFixed(6) +
              '&lon=' + pos.coords.longitude.toFixed(6);
          },
          function () { self.locating = false; self.denied = true; }
        );
      },
    };
  });

  /* ---- search.html: map/filters toggle + locate -------------------------------- */
  Alpine.data('searchFilters', function () {
    return {
      filtersOpen: true,
      mapOpen: false,
      locating: false,
      init: function () { this.mapOpen = window.innerWidth >= 1024; },
      toggleMap: function () { this.mapOpen = !this.mapOpen; },
      toggleFilters: function () { this.filtersOpen = !this.filtersOpen; },
      get resultsClass() {
        return this.mapOpen ? 'lg:col-span-7' : 'lg:col-span-12';
      },
      submitSort: function (e) { e.target.form.requestSubmit(); },
      locate: function () {
        var self = this;
        if (!navigator.geolocation) { return; }
        self.locating = true;
        navigator.geolocation.getCurrentPosition(
          function (pos) {
            window.location =
              '/search?lat=' + pos.coords.latitude.toFixed(6) +
              '&lon=' + pos.coords.longitude.toFixed(6);
          },
          function () { self.locating = false; }
        );
      },
    };
  });

  /* ---- parking_new.html / parking_edit.html: currency symbol + security pick --- */
  Alpine.data('parkingForm', function () {
    return {
      cur: '',
      picked: '',
      syms: { BRL: 'R$', EUR: '€', USD: '$', GBP: '£', JPY: '¥' },
      init: function () {
        if (this.$el.dataset.currency) this.cur = this.$el.dataset.currency;
        if (this.$el.dataset.picked) this.picked = this.$el.dataset.picked;
      },
      get sym() { return this.syms[this.cur.toUpperCase()] || this.cur; },
      updatePicked: function () {
        var checks = this.$el.querySelectorAll('input[type=checkbox]:checked');
        var vals = [];
        for (var i = 0; i < checks.length; i++) vals.push(checks[i].value);
        this.picked = vals.join(',');
      },
    };
  });

  /* ---- parking_details.html: photo lightbox + report triggers ------------------ */
  Alpine.data('detailsPanel', function () {
    return {
      galleryOpen: false,
      gallerySrc: '',
      galleryAlt: '',
      openLightbox: function (e) {
        this.galleryOpen = true;
        this.gallerySrc = e.currentTarget.dataset.src;
        this.galleryAlt = e.currentTarget.dataset.alt;
      },
      closeLightbox: function () { this.galleryOpen = false; },
      report: function (type, id) {
        this.$dispatch('bikenest:report', { type: type, id: id });
      },
    };
  });

  /* ---- parking_details.html: report modal -------------------------------------- */
  Alpine.data('reportModal', function () {
    return {
      open: false,
      type: 'parking',
      tid: '',
      init: function () {
        if (this.$el.dataset.reportId) this.tid = this.$el.dataset.reportId;
      },
      openReport: function (e) {
        this.type = e.detail.type;
        this.tid = e.detail.id;
        this.open = true;
      },
      close: function () { this.open = false; },
      submitClose: function () {
        var self = this;
        setTimeout(function () { self.open = false; }, 600);
      },
    };
  });
});

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

  /* ---- base.html: signed-in account menu (desktop header) ---------------------- */
  Alpine.data('accountMenu', function () {
    return {
      open: false,
      toggle: function () { this.open = !this.open; },
      close: function () { this.open = false; },
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
      syms: { BRL: 'R$', EUR: '€', USD: '$', GBP: '£', JPY: '¥' },
      init: function () {
        if (this.$el.dataset.currency) this.cur = this.$el.dataset.currency;
      },
      get sym() { return this.syms[this.cur.toUpperCase()] || this.cur; },
    };
  });

  /* One day's row in the hours editor. The select is the source of truth (it
   * is what the server reads); this only decides whether that day's two time
   * ranges are worth showing. With Alpine absent they stay visible, which is
   * what makes the no-JS path work. */
  Alpine.data('hoursDay', function () {
    return {
      state: 'unknown',
      init: function () {
        if (this.$el.dataset.state) this.state = this.$el.dataset.state;
      },
      get showRanges() { return this.state === 'ranges'; },
    };
  });

  /* "Copy to all days": copies Monday's five fields onto the other six.
   * Values are written through the DOM and announced with input/change so each
   * row's own x-model picks them up — no shared store to keep in sync. */
  Alpine.data('hoursEditor', function () {
    return {
      suffixes: ['state', '1_open', '1_close', '2_open', '2_close'],
      rest: ['tue', 'wed', 'thu', 'fri', 'sat', 'sun'],
      copyAll: function () {
        var root = this.$root;
        var source = {};
        this.suffixes.forEach(function (suffix) {
          var el = root.querySelector('[name="h_mon_' + suffix + '"]');
          source[suffix] = el ? el.value : '';
        });
        this.rest.forEach(function (day) {
          Object.keys(source).forEach(function (suffix) {
            var el = root.querySelector('[name="h_' + day + '_' + suffix + '"]');
            if (!el) return;
            el.value = source[suffix];
            el.dispatchEvent(new Event('input', { bubbles: true }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
          });
        });
      },
    };
  });

  /* The map picker's controls. The map itself lives in pin-picker.js (it needs
   * MapLibre, which only the pages with a picker load); the two talk over
   * element-scoped events on #pin-map, so nothing leaks across a boosted
   * navigation:
   *
   *   this component --bikenest:pin-set--> #pin-map   (move the pin there)
   *   #pin-map       --bikenest:pin------> this        (the pin moved)
   *
   * With no map at all, the buttons still write the lat/lon inputs directly,
   * so geolocation and address lookup work without MapLibre. */
  Alpine.data('pinPicker', function () {
    return {
      lat: null,
      lon: null,
      locating: false,
      message: '',
      emptyLabel: '',
      locateFailed: '',
      geocodeFailed: '',
      timer: null,
      init: function () {
        var ds = this.$el.dataset;
        this.emptyLabel = ds.empty || '';
        this.locateFailed = ds.locateFailed || '';
        this.geocodeFailed = ds.geocodeFailed || '';
        var current = this.inputs();
        var lat = parseFloat(current.lat && current.lat.value);
        var lon = parseFloat(current.lon && current.lon.value);
        if (isFinite(lat) && isFinite(lon)) { this.lat = lat; this.lon = lon; }
      },
      get coords() {
        if (this.lat === null || this.lon === null) return this.emptyLabel;
        return this.lat.toFixed(6) + ', ' + this.lon.toFixed(6);
      },
      mapEl: function () { return this.$root.querySelector('[data-lat-input]'); },
      inputs: function () {
        var el = this.mapEl();
        var ds = (el && el.dataset) || {};
        return {
          lat: document.getElementById(ds.latInput || 'lat'),
          lon: document.getElementById(ds.lonInput || 'lon'),
        };
      },
      /* The pin moved (drag or map click): mirror it into the readout. The
       * inputs were already written by pin-picker.js. */
      onPin: function (e) {
        var d = (e && e.detail) || {};
        if (!isFinite(d.lat) || !isFinite(d.lon)) return;
        this.lat = d.lat;
        this.lon = d.lon;
        this.message = '';
      },
      setPosition: function (lat, lon) {
        var fields = this.inputs();
        if (fields.lat) fields.lat.value = lat.toFixed(6);
        if (fields.lon) fields.lon.value = lon.toFixed(6);
        this.lat = lat;
        this.lon = lon;
        this.message = '';
        var el = this.mapEl();
        if (el) {
          el.dispatchEvent(new CustomEvent('bikenest:pin-set', {
            detail: { lat: lat, lon: lon },
          }));
        }
      },
      useLocation: function () {
        var self = this;
        if (!navigator.geolocation) { self.message = self.locateFailed; return; }
        self.locating = true;
        navigator.geolocation.getCurrentPosition(
          function (pos) {
            self.locating = false;
            self.setPosition(pos.coords.latitude, pos.coords.longitude);
          },
          function () { self.locating = false; self.message = self.locateFailed; }
        );
      },
      /* Address → position. Never on a keystroke: this reaches a billable
       * provider, so it fires on an explicit tap, or once (debounced) when the
       * address field is left and no position has been picked yet. */
      addressChanged: function () {
        var self = this;
        if (self.lat !== null) return;
        if (self.timer) clearTimeout(self.timer);
        self.timer = setTimeout(function () { self.findAddress(); }, 700);
      },
      findAddress: function () {
        var self = this;
        if (self.timer) { clearTimeout(self.timer); self.timer = null; }
        var field = document.getElementById('address');
        var query = field ? field.value.trim() : '';
        if (!query) return;
        var meta = document.querySelector('meta[name="csrf"]');
        fetch('/api/geocode?q=' + encodeURIComponent(query), {
          headers: meta ? { 'X-CSRF-Token': meta.content } : {},
          credentials: 'same-origin',
        })
          .then(function (res) { return res.ok ? res.json() : null; })
          .then(function (hit) {
            if (!hit || !isFinite(hit.lat) || !isFinite(hit.lon)) {
              self.message = self.geocodeFailed;
              return;
            }
            self.setPosition(hit.lat, hit.lon);
          })
          .catch(function () { self.message = self.geocodeFailed; });
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
      /* htmx 4 fires `htmx:after:request` once the response body has been read
       * and before the swap, with the whole request context on
       * `detail.ctx` (see `#issueRequest` in web/static/vendor/htmx.js:
       * `ctx.response = {raw, status, headers}`). Close only on a 2xx: a
       * refused report (409 duplicate, 429 rate limit, 403) is swapped into
       * `#report-modal-feedback` inside the modal, and closing on submit would
       * have thrown that message away. */
      afterRequest: function (e) {
        var status = e.detail && e.detail.ctx && e.detail.ctx.response
          ? e.detail.ctx.response.status : 0;
        if (status >= 200 && status < 300) this.open = false;
      },
    };
  });

  /* ---- admin_users.html: reveal one masked email ------------------------------- */
  /* The row renders both the masked and the full address; this only flips which
   * one is visible and relabels the button. The label strings come from
   * `data-show`/`data-hide` so the copy stays in the i18n catalog. */
  Alpine.data('revealEmail', function () {
    return {
      shown: false,
      label: '',
      init: function () { this.label = this.showLabel(); },
      showLabel: function () {
        var btn = this.$el.querySelector('button[data-show]');
        return btn ? btn.dataset.show : '';
      },
      hideLabel: function () {
        var btn = this.$el.querySelector('button[data-hide]');
        return btn ? btn.dataset.hide : '';
      },
      toggle: function () {
        this.shown = !this.shown;
        this.label = this.shown ? this.hideLabel() : this.showLabel();
      },
    };
  });
});

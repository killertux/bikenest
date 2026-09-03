/* P2: map rendering + card↔marker sync (plain JS over the server-rendered
 * results). The server stays the source of truth: results arrive as HTML; this
 * file only mirrors them into markers (UI_DESIGN P2, §14, §23).
 *
 * Written to be idempotent so it is safe under hx-boost (whole-body swaps) and
 * HTMX result-fragment swaps: the map is created once and reused; markers are
 * re-rendered from the current #search-data on every (re)load. */
(function () {
  "use strict";

  /* Ledger #3: the style URL and (Mapbox) access token come from the server via
   * <body data-map-style-url / data-map-access-token>. Default: MapLibre demo tiles
   * so the map still renders before MAP_STYLE_URL is configured. */
  var bodyCfg = document.body ? document.body.dataset : {};
  var STYLE_URL = bodyCfg.mapStyleUrl || "https://demotiles.maplibre.org/style.json";
  var ACCESS_TOKEN = bodyCfg.mapAccessToken || "";
  var CENTER_FALLBACK = [-49.2733, -25.4284]; // Curitiba [lon, lat]

  // Per-page-view state, keyed off the #map element so a fresh map (after a
  // boosted navigation swaps in a new #map) starts clean.
  function state(mapEl) {
    if (!mapEl._bn) mapEl._bn = { map: null, markers: {}, dest: null };
    return mapEl._bn;
  }

  function readData() {
    var dataEl = document.getElementById("search-data");
    if (!dataEl) return null;
    try {
      return JSON.parse(dataEl.textContent);
    } catch (e) {
      return null;
    }
  }

  function select(id) {
    document.querySelectorAll("[data-parking-id]").forEach(function (card) {
      card.classList.toggle("selected", Number(card.dataset.parkingId) === id);
    });
    var mapEl = document.getElementById("map");
    if (mapEl && mapEl._bn) {
      Object.keys(mapEl._bn.markers).forEach(function (mid) {
        mapEl._bn.markers[mid].getElement().classList.toggle("selected", Number(mid) === id);
      });
    }
    var card = document.querySelector('[data-parking-id="' + id + '"]');
    if (card) card.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  function renderMarkers(map, st, data) {
    Object.keys(st.markers).forEach(function (id) {
      st.markers[id].remove();
    });
    st.markers = {};
    if (st.dest) {
      st.dest.remove();
      st.dest = null;
    }
    if (data.origin && data.origin.lat != null) {
      var destEl = document.createElement("div");
      destEl.className = "marker marker-destination";
      destEl.title = data.origin.label || "Destination";
      st.dest = new maplibregl.Marker(destEl)
        .setLngLat([data.origin.lon, data.origin.lat])
        .addTo(map);
    }
    (data.items || []).forEach(function (item) {
      var el = document.createElement("div");
      el.className = "marker";
      el.title = item.name;
      el.addEventListener("click", function () {
        select(item.id);
      });
      st.markers[item.id] = new maplibregl.Marker(el)
        .setLngLat([item.lon, item.lat])
        .addTo(map);
    });
  }

  function init() {
    var data = readData();
    if (!data) return;
    var mapEl = document.getElementById("map");
    if (!mapEl || !window.maplibregl) return;

    var st = state(mapEl);
    var center = CENTER_FALLBACK;
    var zoom = 13;
    if (data.origin && data.origin.lat != null) {
      center = [data.origin.lon, data.origin.lat];
      zoom = 14;
    } else if (data.items && data.items.length) {
      center = [data.items[0].lon, data.items[0].lat];
    }

    if (!st.map) {
      if (ACCESS_TOKEN) maplibregl.accessToken = ACCESS_TOKEN;
      st.map = new maplibregl.Map({ container: mapEl, style: STYLE_URL, center: center, zoom: zoom });
      st.map.addControl(new maplibregl.NavigationControl());
      st.map.addControl(new maplibregl.GeolocateControl());
      var recenter = document.getElementById("recenter");
      if (recenter) recenter.addEventListener("click", function () { st.map.flyTo({ center: center, zoom: zoom }); });
      st.map.on("load", function () { renderMarkers(st.map, st, readData() || data); });
    } else {
      st.map.jumpTo({ center: center, zoom: zoom });
      renderMarkers(st.map, st, data);
    }

    var countEl = document.getElementById("map-count");
    if (countEl) countEl.textContent = (data.items || []).length + " on map";
  }

  // Bind delegated + HTMX listeners exactly once (survives script re-execution
  // under hx-boost).
  if (!window.__bnSearchBound) {
    window.__bnSearchBound = true;
    document.addEventListener("click", function (e) {
      var card = e.target.closest("[data-parking-id]");
      if (card) select(Number(card.dataset.parkingId));
    });
    // Re-render markers after an HTMX results-fragment swap (htmx 4 event).
    document.body.addEventListener("htmx:after:swap", function () {
      setTimeout(init, 0);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();

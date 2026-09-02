/* P3: single-location map (UI_DESIGN P3). */
(function () {
  "use strict";
  function init() {
    var el = document.getElementById("map-single");
    if (!el || !window.maplibregl || el.dataset.initialized) return;
    el.dataset.initialized = "1";
    var lat = parseFloat(el.dataset.lat);
    var lon = parseFloat(el.dataset.lon);
    var map = new maplibregl.Map({
      container: el.id,
      style: "https://demotiles.maplibre.org/style.json", // dev tiles — Ledger #3
      center: [lon, lat],
      zoom: 17,
    });
    map.addControl(new maplibregl.NavigationControl());
    map.on("load", function () {
      var markerEl = document.createElement("div");
      markerEl.className = "marker";
      new maplibregl.Marker(markerEl).setLngLat([lon, lat]).addTo(map);
    });
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();

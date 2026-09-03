/* P3: single-location map. */
(function () {
  "use strict";
  /* Ledger #3: style URL + (Mapbox) access token from <body data-*>; default demo tiles. */
  var bodyCfg = document.body ? document.body.dataset : {};
  var STYLE_URL = bodyCfg.mapStyleUrl || "https://demotiles.maplibre.org/style.json";
  var ACCESS_TOKEN = bodyCfg.mapAccessToken || "";
  function init() {
    var el = document.getElementById("map-single");
    if (!el || !window.maplibregl || el.dataset.initialized) return;
    el.dataset.initialized = "1";
    var lat = parseFloat(el.dataset.lat);
    var lon = parseFloat(el.dataset.lon);
    if (ACCESS_TOKEN) maplibregl.accessToken = ACCESS_TOKEN;
    var map = new maplibregl.Map({
      container: el.id,
      style: STYLE_URL,
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

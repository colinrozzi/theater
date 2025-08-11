// Populate the sidebar
//
// This is a script, and not included directly in the page, to control the total size of the book.
// The TOC contains an entry for each page, so if each page includes a copy of the TOC,
// the total size of the page becomes O(n**2).
class MDBookSidebarScrollbox extends HTMLElement {
    constructor() {
        super();
    }
    connectedCallback() {
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><a href="index.html">Theater Documentation</a></li><li class="chapter-item expanded affix "><li class="part-title">Introduction</li><li class="chapter-item expanded "><a href="introduction/index.html"><strong aria-hidden="true">1.</strong> Overview</a></li><li class="chapter-item expanded "><a href="introduction/why-theater.html"><strong aria-hidden="true">2.</strong> Why Theater?</a></li><li class="chapter-item expanded affix "><li class="part-title">Core Concepts</li><li class="chapter-item expanded "><a href="core-concepts/index.html"><strong aria-hidden="true">3.</strong> Overview</a></li><li class="chapter-item expanded "><a href="core-concepts/wasm-components.html"><strong aria-hidden="true">4.</strong> WebAssembly Components &amp; Sandboxing</a></li><li class="chapter-item expanded "><a href="core-concepts/actor-model.html"><strong aria-hidden="true">5.</strong> Actor Model &amp; Supervision</a></li><li class="chapter-item expanded "><a href="core-concepts/traceability.html"><strong aria-hidden="true">6.</strong> Traceability &amp; Verification</a></li><li class="chapter-item expanded affix "><li class="part-title">User Guide</li><li class="chapter-item expanded "><a href="user-guide/configuration.html"><strong aria-hidden="true">7.</strong> Configuration</a></li><li class="chapter-item expanded "><a href="user-guide/cli.html"><strong aria-hidden="true">8.</strong> CLI</a></li><li class="chapter-item expanded "><a href="user-guide/troubleshooting.html"><strong aria-hidden="true">9.</strong> Troubleshooting</a></li><li class="chapter-item expanded affix "><li class="part-title">Use Cases</li><li class="chapter-item expanded "><a href="use-cases/ai-agents.html"><strong aria-hidden="true">10.</strong> Building AI Agent Systems</a></li><li class="chapter-item expanded "><a href="use-cases/ai-generated-code.html"><strong aria-hidden="true">11.</strong> Running AI-Generated Code</a></li><li class="chapter-item expanded affix "><li class="part-title">Development</li><li class="chapter-item expanded "><a href="development/building-actors.html"><strong aria-hidden="true">12.</strong> Building Actors</a></li><li class="chapter-item expanded "><a href="development/building-host-functions.html"><strong aria-hidden="true">13.</strong> Building Host Functions</a></li><li class="chapter-item expanded "><a href="development/making-changes.html"><strong aria-hidden="true">14.</strong> Making Changes</a></li><li class="chapter-item expanded "><a href="development/system-internals/index.html"><strong aria-hidden="true">15.</strong> System Internals</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="development/system-internals/architecture.html"><strong aria-hidden="true">15.1.</strong> System Architecture</a></li><li class="chapter-item expanded "><a href="development/system-internals/components.html"><strong aria-hidden="true">15.2.</strong> Component Relationships</a></li><li class="chapter-item expanded "><a href="development/system-internals/data-flow.html"><strong aria-hidden="true">15.3.</strong> Data Flow</a></li><li class="chapter-item expanded "><a href="development/system-internals/implementation.html"><strong aria-hidden="true">15.4.</strong> Implementation Details</a></li><li class="chapter-item expanded "><a href="development/system-internals/actor-ids.html"><strong aria-hidden="true">15.5.</strong> Actor ID System</a></li><li class="chapter-item expanded "><a href="development/system-internals/interface-system.html"><strong aria-hidden="true">15.6.</strong> Interface System</a></li></ol></li><li class="chapter-item expanded "><li class="part-title">Services</li><li class="chapter-item expanded "><a href="services/handlers.html"><strong aria-hidden="true">16.</strong> Handlers</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="services/handlers/index.html"><strong aria-hidden="true">16.1.</strong> Handler System</a></li><li class="chapter-item expanded "><a href="services/handlers/message-server.html"><strong aria-hidden="true">16.2.</strong> Message Server</a></li><li class="chapter-item expanded "><a href="services/handlers/http-client.html"><strong aria-hidden="true">16.3.</strong> HTTP Client</a></li><li class="chapter-item expanded "><a href="services/handlers/http-framework.html"><strong aria-hidden="true">16.4.</strong> HTTP Framework</a></li><li class="chapter-item expanded "><a href="services/handlers/filesystem.html"><strong aria-hidden="true">16.5.</strong> Filesystem</a></li><li class="chapter-item expanded "><a href="services/handlers/supervisor.html"><strong aria-hidden="true">16.6.</strong> Supervisor</a></li><li class="chapter-item expanded "><a href="services/handlers/store.html"><strong aria-hidden="true">16.7.</strong> Store</a></li><li class="chapter-item expanded "><a href="services/handlers/runtime.html"><strong aria-hidden="true">16.8.</strong> Runtime</a></li><li class="chapter-item expanded "><a href="services/handlers/timing.html"><strong aria-hidden="true">16.9.</strong> Timing</a></li></ol></li><li class="chapter-item expanded "><li class="part-title">API Reference</li><li class="chapter-item expanded "><a href="api-reference/api.html"><strong aria-hidden="true">17.</strong> API Documentation</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0].split("?")[0];
        if (current_page.endsWith("/")) {
            current_page += "index.html";
        }
        var links = Array.prototype.slice.call(this.querySelectorAll("a"));
        var l = links.length;
        for (var i = 0; i < l; ++i) {
            var link = links[i];
            var href = link.getAttribute("href");
            if (href && !href.startsWith("#") && !/^(?:[a-z+]+:)?\/\//.test(href)) {
                link.href = path_to_root + href;
            }
            // The "index" page is supposed to alias the first chapter in the book.
            if (link.href === current_page || (i === 0 && path_to_root === "" && current_page.endsWith("/index.html"))) {
                link.classList.add("active");
                var parent = link.parentElement;
                if (parent && parent.classList.contains("chapter-item")) {
                    parent.classList.add("expanded");
                }
                while (parent) {
                    if (parent.tagName === "LI" && parent.previousElementSibling) {
                        if (parent.previousElementSibling.classList.contains("chapter-item")) {
                            parent.previousElementSibling.classList.add("expanded");
                        }
                    }
                    parent = parent.parentElement;
                }
            }
        }
        // Track and set sidebar scroll position
        this.addEventListener('click', function(e) {
            if (e.target.tagName === 'A') {
                sessionStorage.setItem('sidebar-scroll', this.scrollTop);
            }
        }, { passive: true });
        var sidebarScrollTop = sessionStorage.getItem('sidebar-scroll');
        sessionStorage.removeItem('sidebar-scroll');
        if (sidebarScrollTop) {
            // preserve sidebar scroll position when navigating via links within sidebar
            this.scrollTop = sidebarScrollTop;
        } else {
            // scroll sidebar to current active section when navigating via "next/previous chapter" buttons
            var activeSection = document.querySelector('#sidebar .active');
            if (activeSection) {
                activeSection.scrollIntoView({ block: 'center' });
            }
        }
        // Toggle buttons
        var sidebarAnchorToggles = document.querySelectorAll('#sidebar a.toggle');
        function toggleSection(ev) {
            ev.currentTarget.parentElement.classList.toggle('expanded');
        }
        Array.from(sidebarAnchorToggles).forEach(function (el) {
            el.addEventListener('click', toggleSection);
        });
    }
}
window.customElements.define("mdbook-sidebar-scrollbox", MDBookSidebarScrollbox);

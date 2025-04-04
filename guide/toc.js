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
        this.innerHTML = '<ol class="chapter"><li class="chapter-item expanded affix "><a href="index.html">Theater Documentation</a></li><li class="chapter-item expanded affix "><li class="part-title">Introduction</li><li class="chapter-item expanded "><a href="introduction/index.html"><strong aria-hidden="true">1.</strong> Overview</a></li><li class="chapter-item expanded "><a href="introduction/why-theater.html"><strong aria-hidden="true">2.</strong> Why Theater?</a></li><li class="chapter-item expanded affix "><li class="part-title">Core Concepts</li><li class="chapter-item expanded "><a href="core-concepts/architecture.html"><strong aria-hidden="true">3.</strong> Architecture</a></li><li class="chapter-item expanded "><a href="core-concepts/actors.html"><strong aria-hidden="true">4.</strong> Actors</a></li><li class="chapter-item expanded "><a href="core-concepts/actor-ids.html"><strong aria-hidden="true">5.</strong> Actor IDs</a></li><li class="chapter-item expanded "><a href="core-concepts/event-chain.html"><strong aria-hidden="true">6.</strong> Event Chain</a></li><li class="chapter-item expanded "><a href="core-concepts/state-management.html"><strong aria-hidden="true">7.</strong> State Management</a></li><li class="chapter-item expanded "><a href="core-concepts/supervision.html"><strong aria-hidden="true">8.</strong> Supervision</a></li><li class="chapter-item expanded "><a href="core-concepts/interface-system.html"><strong aria-hidden="true">9.</strong> Interface System</a></li><li class="chapter-item expanded "><a href="core-concepts/store/index.html"><strong aria-hidden="true">10.</strong> Store System</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="core-concepts/store/actor-api.html"><strong aria-hidden="true">10.1.</strong> Actor API</a></li><li class="chapter-item expanded "><a href="core-concepts/store/usage-patterns.html"><strong aria-hidden="true">10.2.</strong> Usage Patterns</a></li></ol></li><li class="chapter-item expanded "><li class="part-title">User Guide</li><li class="chapter-item expanded "><a href="user-guide/configuration.html"><strong aria-hidden="true">11.</strong> Configuration</a></li><li class="chapter-item expanded "><a href="user-guide/cli.html"><strong aria-hidden="true">12.</strong> CLI</a></li><li class="chapter-item expanded "><a href="user-guide/troubleshooting.html"><strong aria-hidden="true">13.</strong> Troubleshooting</a></li><li class="chapter-item expanded affix "><li class="part-title">Development</li><li class="chapter-item expanded "><a href="development/building-actors.html"><strong aria-hidden="true">14.</strong> Building Actors</a></li><li class="chapter-item expanded "><a href="development/building-host-functions.html"><strong aria-hidden="true">15.</strong> Building Host Functions</a></li><li class="chapter-item expanded "><a href="development/making-changes.html"><strong aria-hidden="true">16.</strong> Making Changes</a></li><li class="chapter-item expanded affix "><li class="part-title">Services</li><li class="chapter-item expanded "><a href="services/handlers.html"><strong aria-hidden="true">17.</strong> Handlers</a></li><li><ol class="section"><li class="chapter-item expanded "><a href="services/handlers/index.html"><strong aria-hidden="true">17.1.</strong> Handler System</a></li><li class="chapter-item expanded "><a href="services/handlers/message-server.html"><strong aria-hidden="true">17.2.</strong> Message Server</a></li><li class="chapter-item expanded "><a href="services/handlers/http-client.html"><strong aria-hidden="true">17.3.</strong> HTTP Client</a></li><li class="chapter-item expanded "><a href="services/handlers/http-framework.html"><strong aria-hidden="true">17.4.</strong> HTTP Framework</a></li><li class="chapter-item expanded "><a href="services/handlers/filesystem.html"><strong aria-hidden="true">17.5.</strong> Filesystem</a></li><li class="chapter-item expanded "><a href="services/handlers/supervisor.html"><strong aria-hidden="true">17.6.</strong> Supervisor</a></li><li class="chapter-item expanded "><a href="services/handlers/store.html"><strong aria-hidden="true">17.7.</strong> Store</a></li><li class="chapter-item expanded "><a href="services/handlers/runtime.html"><strong aria-hidden="true">17.8.</strong> Runtime</a></li><li class="chapter-item expanded "><a href="services/handlers/timing.html"><strong aria-hidden="true">17.9.</strong> Timing</a></li></ol></li><li class="chapter-item expanded "><li class="part-title">API Reference</li><li class="chapter-item expanded "><a href="api-reference/api.html"><strong aria-hidden="true">18.</strong> API Documentation</a></li></ol>';
        // Set the current, active page, and reveal it if it's hidden
        let current_page = document.location.href.toString().split("#")[0];
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

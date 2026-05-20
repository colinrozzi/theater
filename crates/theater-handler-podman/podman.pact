// Podman Handler Interface
//
// Container management via the `podman` CLI. Lets actors spin up, stop,
// and enumerate containers — the substrate for orchestrating agent
// containers (agentry et al).
//
// Implementation shells out to the podman binary, so no daemon is
// required: just podman installed on the host. Rootless or rootful
// modes both work.

interface podman {
    @package: string = "theater:simple"

    // Bind-mount of a host path into the container.
    record mount-spec {
        source: string,
        target: string,
        read-only: bool,
    }

    // Everything needed to start a container.
    record container-spec {
        // OCI image reference, e.g. "localhost/agent-poc:latest".
        image: string,
        // Container name. Must be unique within the host.
        name: string,
        // Environment variables (key, value pairs).
        env: list<tuple<string, string>>,
        // Bind-mounts.
        mounts: list<mount-spec>,
        // Override the image's default command.
        cmd: option<list<string>>,
        // Allocate a pseudo-TTY (equivalent to `podman run -t`).
        tty: bool,
        // Keep STDIN open (equivalent to `podman run -i`).
        interactive: bool,
    }

    // One entry from `podman ps -a`.
    record container-info {
        id: string,
        name: string,
        image: string,
        // Podman status string: "running", "exited", "created", "paused", etc.
        status: string,
        // Exit code if the container has exited; none while running.
        exit-code: option<s32>,
    }

    exports {
        // Start a container detached. Returns the container ID on success.
        run: func(spec: container-spec) -> result<string, string>

        // Stop a container by name. Sends SIGTERM, waits up to 10s, then
        // SIGKILL. Returns ok() if the container was already stopped or
        // doesn't exist.
        stop: func(name: string) -> result<_, string>

        // Remove a stopped container by name. `force=true` also kills
        // running containers before removal.
        rm: func(name: string, force: bool) -> result<_, string>

        // Enumerate all containers (running + stopped) on the host.
        list: func() -> result<list<container-info>, string>
    }
}

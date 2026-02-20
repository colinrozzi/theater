// Logger interface

interface logger {
    @version: string = "1.0.0"

    exports {
        log: func(msg: string)
    }
}

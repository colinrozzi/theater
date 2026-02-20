// Calculator interface - demonstrates basic Pact features

interface calculator {
    @version: string = "1.0.0"

    record point {
        x: f32,
        y: f32,
    }

    imports {
        logger
    }

    exports {
        add: func(a: s32, b: s32) -> s32
        sub: func(a: s32, b: s32) -> s32
    }
}

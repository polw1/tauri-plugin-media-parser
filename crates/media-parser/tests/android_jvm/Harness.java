package com.plugin.mediaparser;

/** Runs the real Rust crate in an Android JVM, preserving failure exit codes. */
public final class Harness {
    private static native int init(Class<?> streamClass);
    private static native int run(String mode);

    public static void main(String[] args) {
        try {
            if (args.length != 2) throw new IllegalArgumentException("library and mode required");
            System.load(args[0]);
            String mode = args[1];
            if (mode.equals("failed-bootstrap")) {
                if (init(String.class) == 0) throw new AssertionError("invalid bootstrap accepted");
                System.exit(run("missing-runtime"));
            }
            if (mode.equals("missing-runtime")) System.exit(run(mode));
            if (!mode.equals("normal") && !mode.equals("negative")) {
                throw new IllegalArgumentException("unknown mode " + mode);
            }
            if (mode.equals("normal")) BoundedStreamCases.run();
            if (init(FaultInjectingStream.class) != 0) throw new AssertionError("bootstrap failed");
            System.exit(run(mode));
        } catch (Throwable failure) {
            failure.printStackTrace();
            System.exit(1);
        }
    }
}

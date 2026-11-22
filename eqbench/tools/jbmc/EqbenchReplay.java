import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.util.Arrays;

/**
 * Runs {@code eqbench.Miter.check} on the inputs of a JBMC counterexample, to
 * confirm it on a real JVM: JBMC's models of the Java library are not exact,
 * e.g. {@code Math.log} returns any value.
 *
 * <pre>java -ea -cp miter-classes:eqbench-replay.jar EqbenchReplay bits...</pre>
 *
 * Each argument is the bits of a parameter, as JBMC's trace prints them.
 * Prints {@code differ} if the miter's assertion fails, {@code same} otherwise,
 * followed by the inputs.
 */
public class EqbenchReplay {
    public static void main(String[] bits) throws Throwable {
        Method check = null;
        for (Method method : Class.forName("eqbench.Miter").getMethods()) {
            if (method.getName().equals("check")) {
                check = method;
            }
        }
        Class<?>[] types = check.getParameterTypes();
        Object[] args = new Object[types.length];
        for (int i = 0; i < types.length; i++) {
            args[i] = value(types[i], Long.parseUnsignedLong(bits[i], 2));
        }
        String verdict = "same";
        try {
            check.invoke(null, args);
        } catch (InvocationTargetException e) {
            if (!(e.getCause() instanceof AssertionError)) {
                throw e.getCause();
            }
            verdict = "differ";
        }
        System.out.println(verdict + " on " + Arrays.toString(args));
    }

    private static Object value(Class<?> type, long bits) {
        if (type == int.class) return (int) bits;
        if (type == long.class) return bits;
        if (type == double.class) return Double.longBitsToDouble(bits);
        if (type == float.class) return Float.intBitsToFloat((int) bits);
        if (type == boolean.class) return bits != 0;
        if (type == char.class) return (char) bits;
        if (type == short.class) return (short) bits;
        if (type == byte.class) return (byte) bits;
        throw new IllegalArgumentException("unsupported parameter type " + type);
    }
}

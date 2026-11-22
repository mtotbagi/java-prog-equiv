import com.code_intelligence.jazzer.api.FuzzedDataProvider;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;

/**
 * Fuzz target of any eqbench miter: calls {@code eqbench.Miter.check} on
 * inputs drawn from the fuzzer. The miter's {@code AssertionError}, thrown
 * when the two versions disagree, is the finding.
 */
public class EqbenchFuzzTarget {
    private static final Method CHECK = check();

    private static Method check() {
        try {
            for (Method method : Class.forName("eqbench.Miter").getMethods()) {
                if (method.getName().equals("check")) {
                    return method;
                }
            }
        } catch (ClassNotFoundException e) {
            throw new IllegalStateException("eqbench.Miter is not on the class path", e);
        }
        throw new IllegalStateException("eqbench.Miter has no check method");
    }

    public static void fuzzerTestOneInput(FuzzedDataProvider data) throws Throwable {
        Class<?>[] types = CHECK.getParameterTypes();
        Object[] args = new Object[types.length];
        for (int i = 0; i < types.length; i++) {
            args[i] = value(data, types[i]);
        }
        try {
            CHECK.invoke(null, args);
        } catch (InvocationTargetException e) {
            throw e.getCause();
        }
    }

    private static Object value(FuzzedDataProvider data, Class<?> type) {
        if (type == int.class) return data.consumeInt();
        if (type == long.class) return data.consumeLong();
        if (type == double.class) return data.consumeDouble();
        if (type == float.class) return data.consumeFloat();
        if (type == boolean.class) return data.consumeBoolean();
        if (type == char.class) return data.consumeChar();
        if (type == short.class) return data.consumeShort();
        if (type == byte.class) return data.consumeByte();
        throw new IllegalArgumentException("unsupported parameter type " + type);
    }
}

# kotlinx.serialization keeps its serializers as synthetic members of the
# serializable class; R8 cannot see the reference and strips them, and the
# failure is a runtime "Serializer for class X not found" rather than a
# build error.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**
-keepclassmembers class jp.golia.mailrs.wire.** {
    *** Companion;
}
-keepclasseswithmembers class jp.golia.mailrs.wire.** {
    kotlinx.serialization.KSerializer serializer(...);
}

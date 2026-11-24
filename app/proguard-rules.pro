# JNI: the native library calls back into the service by method name, so R8 must not rename or remove them.
-keep interface dev.facetilt.engine.DecisionListener { *; }
-keepclassmembers class * implements dev.facetilt.engine.DecisionListener {
    public void onOrientationDecision(int, int);
    public void onAssistControl(boolean);
}
-keepclasseswithmembernames class dev.facetilt.engine.NativeBridge { native <methods>; }

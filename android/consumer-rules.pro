-keep class com.plugin.mediaparser.BoundedJpegOutputStream {
    public <init>(int);
    public int failure;
    public int size;
    public void write(int);
    public void write(byte[], int, int);
    public byte[] toByteArray();
}

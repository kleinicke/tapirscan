package org.tapirscan.android;

/** Source-image pixel coordinates, origin top-left, x rightward and y downward. */
public final class Point {
    private final double x;
    private final double y;

    Point(double x, double y) {
        this.x = x;
        this.y = y;
    }

    public double x() {
        return x;
    }

    public double y() {
        return y;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Point && x == ((Point) other).x && y == ((Point) other).y;
    }

    @Override
    public int hashCode() {
        return Double.hashCode(x) * 31 + Double.hashCode(y);
    }

    @Override
    public String toString() {
        return "(" + x + ", " + y + ")";
    }
}

ioreg -c AppleARMIODevice -r -d 1 | \
grep "voltage-states" | \
perl -ne 'if (/voltage-states(\d+)/) {
    print "voltage-states$1: ";
    if (/<([0-9a-f\s]+)>/) {
        my @pairs = ($1 =~ m/([0-9a-f]{8}[0-9a-f]{8})/g);
        foreach my $pair (@pairs) {
            my $freq = substr($pair, 0, 8);
            $mhz = hex($freq) / 1000000;
            printf "%.0f MHz, ", $mhz;
        }
        print "\n";
    }
}'
